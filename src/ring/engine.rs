use futures_core::ready;

use std::io;
use std::mem;
use std::os::unix::io::RawFd;
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Context, Poll};

use crate::event::Cancellation;
use crate::driver::{Drive, Completion};

use State::*;

pub struct Engine<D> {
    state: State,
    completion: NonNull<Completion>,
    driver: D,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum State {
    Inert,
    WaitingRead,
    PreparedRead,
    SubmittedRead,
    WaitingWrite,
    PreparedWrite,
    SubmittedWrite,
}

impl<D: Drive> Engine<D> {
    pub fn new(driver: D) -> Engine<D> {
        Engine {
            state: Inert,
            completion: NonNull::dangling(),
            driver,
        }
    }

    /// Safety: caller must ensure unique ownership of both fd and the backing buffer
    pub unsafe fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        fd: RawFd,
        buf: &mut [u8]
    ) -> Poll<io::Result<usize>> {
        match self.state {
            Inert | WaitingRead                             => {
                *self.as_mut().state() = WaitingRead;
                ready!(self.as_mut().try_prepare(ctx, fd, buf, |sqe, ctx, io, buf| {
                    prepare_read(sqe, ctx, io, buf)
                }));
                ready!(self.as_mut().try_submit(ctx));
                Poll::Pending
            }
            PreparedRead                                    => {
                match self.as_mut().try_complete(ctx) {
                    ready @ Poll::Ready(..) => ready,
                    Poll::Pending           => {
                        ready!(self.as_mut().try_submit(ctx));
                        Poll::Pending
                    }
                }
            }
            SubmittedRead                                   => {
                self.as_mut().try_complete(ctx)
            }
            WaitingWrite | PreparedWrite | SubmittedWrite   => {
                panic!("attempted simultaneous read and write on same object")
            }
        }
    }

    /// Safety: caller must ensure unique ownership of both fd and the backing buffer
    #[allow(dead_code)]
    pub unsafe fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        fd: RawFd,
        buf: &[u8]
    ) -> Poll<io::Result<usize>> {
        match self.state {
            Inert | WaitingWrite                        => {
                *self.as_mut().state() = WaitingWrite;
                ready!(self.as_mut().try_prepare(ctx, fd, buf, |sqe, ctx, io, buf| {
                    prepare_write(sqe, ctx, io, buf)
                }));
                ready!(self.as_mut().try_submit(ctx));
                Poll::Pending
            }
            PreparedWrite                               => {
                match self.as_mut().try_complete(ctx) {
                    ready @ Poll::Ready(..) => ready,
                    Poll::Pending           => {
                        ready!(self.as_mut().try_submit(ctx));
                        Poll::Pending
                    }
                }
            }
            SubmittedWrite                              => {
                self.as_mut().try_complete(ctx)
            }
            WaitingRead | PreparedRead | SubmittedRead  => {
                panic!("attempted simultaneous read and write on same object")
            }
        }
    }

    #[inline(always)]
    unsafe fn try_prepare<T, F>(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        fd: RawFd,
        buf: T,
        prepare: F,
    ) -> Poll<()> where
        F: FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'_>, RawFd, T) -> NonNull<Completion>
    {
        let driver = self.as_mut().driver();
        let completion = ready!(driver.poll_prepare(ctx, |sqe, ctx| prepare(sqe, ctx, fd, buf)));
        *self.as_mut().completion() = completion;
        self.as_mut().state().prepare();
        Poll::Ready(())
    }

    #[inline(always)]
    unsafe fn try_submit(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        // TODO figure out how to handle this result
        let _ = ready!(self.as_mut().driver().poll_submit(ctx, true));
        self.as_mut().state().submit();
        Poll::Pending
    }

    #[inline(always)]
    unsafe fn try_complete(mut self: Pin<&mut Self>, ctx: &mut Context<'_>)
        -> Poll<io::Result<usize>>
    {
        if let Some(result) = self.as_mut().completion().as_ref().check() {
            *self.as_mut().state() = Inert;
            drop(Box::<Completion>::from_raw(self.as_mut().completion().as_ptr()));
            Poll::Ready(result)
        } else {
            self.as_mut().completion().as_ref().set_waker(ctx.waker().clone());
            Poll::Pending
        }
    }

    #[inline(always)]
    fn driver(self: Pin<&mut Self>) -> Pin<&mut D> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.driver) }
    }

    #[inline(always)]
    fn state(self: Pin<&mut Self>) -> &mut State {
        unsafe { &mut Pin::get_unchecked_mut(self).state }
    }

    #[inline(always)]
    fn completion(self: Pin<&mut Self>) -> &mut NonNull<Completion> {
        unsafe { &mut Pin::get_unchecked_mut(self).completion }
    }
}


impl<D> Engine<D> {
    // this must be the correct cancellation callback for the event currently being driven
    pub unsafe fn cancel(&mut self, cancellation: Cancellation) {
        if matches!(self.state, PreparedRead | PreparedWrite | SubmittedRead | SubmittedWrite) {
            self.completion.as_ref().cancel(cancellation);
        }
    }
}

impl State {
    #[inline(always)]
    fn prepare(&mut self) {
        match self {
            WaitingRead     => *self = PreparedRead,
            WaitingWrite    => *self = PreparedWrite,
            _               => (),
        }
    }
    #[inline(always)]
    fn submit(&mut self) {
        match self {
            PreparedRead    => *self = SubmittedRead,
            PreparedWrite   => *self = SubmittedWrite,
            _               => (),
        }
    }
}

unsafe fn prepare_read(
    sqe: iou::SubmissionQueueEvent<'_>,
    ctx: &mut Context<'_>, 
    fd: RawFd,
    buf: &mut [u8],
) -> NonNull<Completion> {
    let mut sqe = SubmissionCleaner(sqe);
    sqe.0.prep_read(fd, buf, 0);

    let completion = Box::new(Completion::new(ctx.waker().clone()));
    let completion = NonNull::new_unchecked(Box::into_raw(completion));
    sqe.0.set_user_data(completion.as_ptr() as usize as u64);
    mem::forget(sqe);
    completion
}

unsafe fn prepare_write(
    sqe: iou::SubmissionQueueEvent<'_>,
    ctx: &mut Context<'_>, 
    fd: RawFd,
    buf: &[u8],
) -> NonNull<Completion> {
    let mut sqe = SubmissionCleaner(sqe);
    sqe.0.prep_write(fd, buf, 0);

    let completion = Box::new(Completion::new(ctx.waker().clone()));
    let completion = NonNull::new_unchecked(Box::into_raw(completion));
    sqe.0.set_user_data(completion.as_ptr() as usize as u64);
    mem::forget(sqe);
    completion
}

// Use the SubmissionCleaner guard to clear the submission of any data
// in case the Event::prepare method panics
struct SubmissionCleaner<'a>(iou::SubmissionQueueEvent<'a>);

impl<'a> Drop for SubmissionCleaner<'a> {
    fn drop(&mut self) {
        unsafe {
            self.0.prep_nop();
            self.0.set_user_data(0);
        }
    }
}
