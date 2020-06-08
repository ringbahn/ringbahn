use std::io;
use std::os::unix::io::RawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::completion::Completion;
use crate::drive::Completion as ExternalCompletion;
use crate::drive::Drive;
use crate::event::Cancellation;

use State::*;

pub struct Engine<Ops, D: Drive> {
    state: State,
    active: Option<Ops>,
    fd: RawFd,
    completion: Option<Completion>,
    driver: D,
}

#[derive(Debug, Eq, PartialEq)]
enum State {
    Inert = 0,
    Prepared,
    Submitted,
    Lost,
}

impl<Ops, D: Drive> Engine<Ops, D> {
    #[inline(always)]
    pub fn new(fd: RawFd, driver: D) -> Engine<Ops, D> {
        Engine {
            state: Inert,
            active: None,
            completion: None,
            fd, driver,
        }
    }

    #[inline(always)]
    pub fn active(&self) -> Option<&Ops> {
        self.active.as_ref()
    }

    #[inline(always)]
    pub fn set_active(self: Pin<&mut Self>, op: Ops) {
        unsafe { Pin::get_unchecked_mut(self).active = Some(op) }
    }

    #[inline(always)]
    pub fn unset_active(self: Pin<&mut Self>) {
        unsafe { Pin::get_unchecked_mut(self).active = None }
    }

    #[inline(always)]
    pub fn fd(&self) -> RawFd {
        self.fd
    }

    #[inline]
    pub unsafe fn poll(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(&mut iou::SubmissionQueueEvent<'_>, RawFd),
    ) -> Poll<io::Result<usize>> {
        match self.state {
            Inert       => {
                ready!(self.as_mut().try_prepare(ctx, prepare));
                ready!(self.as_mut().try_submit(ctx));
                Poll::Pending
            }
            Prepared    => {
                match self.as_mut().try_complete(ctx) {
                    ready @ Poll::Ready(..) => ready,
                    Poll::Pending           => {
                        ready!(self.as_mut().try_submit(ctx));
                        Poll::Pending
                    }
                }
            }
            Submitted   => self.try_complete(ctx),
            Lost        => panic!("engine in a bad state; driver is faulty"),
        }
    }

    #[inline(always)]
    unsafe fn try_prepare(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(&mut iou::SubmissionQueueEvent<'_>, RawFd),
    ) -> Poll<()> {
        let fd = self.fd;
        let (driver, mut state) = self.as_mut().driver_and_state();
        let completion = ready!(driver.poll_prepare(ctx, |mut sqe, ctx| {
            *state = Lost;
            prepare(&mut sqe, fd);
            let completion = Completion::new(ctx.waker().clone());
            sqe.set_user_data(completion.addr());
            ExternalCompletion::new(completion, ctx)
        }));
        *state = Prepared;
        *self.completion() = Some(completion.real);
        Poll::Ready(())
    }

    #[inline(always)]
    unsafe fn try_submit(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        let (driver, mut state) = self.driver_and_state();
        // TODO figure out how to handle this result
        let _ = ready!(driver.poll_submit(ctx, true));
        *state = Submitted;
        Poll::Ready(())
    }

    #[inline(always)]
    unsafe fn try_complete(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if let Some(result) = self.completion.as_ref().and_then(|c| c.check()) {
            *self.as_mut().state() = Inert;
            self.as_mut().completion().take().unwrap().deallocate();
            Poll::Ready(result)
        } else {
            if let Some(completion) = &self.completion {
                completion.set_waker(ctx.waker().clone());
            }
            Poll::Pending
        }
    }

    #[inline]
    pub fn cancel(self: Pin<&mut Self>, mut cancellation: Cancellation) {
        unsafe {
            if let Some(completion) = self.completion().take() {
                completion.cancel(cancellation);
            } else {
                cancellation.cancel();
            }
        }
    }

    #[inline(always)]
    fn driver_and_state(self: Pin<&mut Self>) -> (Pin<&mut D>, Pin<&mut State>) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.driver), Pin::new_unchecked(&mut this.state))
        }
    }

    #[inline(always)]
    fn state(self: Pin<&mut Self>) -> Pin<&mut State> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.state) }
    }

    #[inline(always)]
    fn completion(self: Pin<&mut Self>) -> Pin<&mut Option<Completion>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.completion) }
    }
}
