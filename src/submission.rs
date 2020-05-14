use std::future::Future;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::{Event, Drive};
use crate::driver::Completion;

pub struct Submission<E: Event, D> {
    state: State,
    event: ManuallyDrop<E>,
    driver: ManuallyDrop<D>,
    completion: NonNull<Completion>,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum State {
    Waiting,
    Prepared,
    Submitted,
    Complete,
}

impl<E: Event, D: Drive> Submission<E, D> {
    pub fn new(event: E, driver: D) -> Submission<E, D> {
        Submission {
            state: State::Waiting,
            event: ManuallyDrop::new(event),
            driver: ManuallyDrop::new(driver),
            completion: NonNull::dangling(),
        }
    }

    #[inline(always)]
    unsafe fn try_prepare(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        let (event, driver) = self.as_mut().event_and_driver();
        let completion = ready!(driver.poll_prepare(ctx, |sqe, ctx| prepare(sqe, ctx, event)));
        let this = Pin::get_unchecked_mut(self);
        this.state = State::Prepared;
        this.completion = completion;
        Poll::Ready(())
    }

    #[inline(always)]
    unsafe fn try_submit(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        let (event, driver) = self.as_mut().event_and_driver();
        // TODO figure out how to handle this result
        let _ = ready!(driver.poll_submit(ctx, event.is_eager()));
        Pin::get_unchecked_mut(self).state = State::Submitted;
        Poll::Pending
    }

    #[inline(always)]
    unsafe fn try_complete(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<(E, io::Result<usize>)> {
        let this = Pin::get_unchecked_mut(self);
        if let Some(result) = this.completion.as_ref().check() {
            this.state = State::Complete;
            drop(Box::<Completion>::from_raw(this.completion.as_ptr()));
            ManuallyDrop::drop(&mut this.driver);
            let event = ManuallyDrop::take(&mut this.event);
            Poll::Ready((event, result))
        } else {
            this.completion.as_ref().set_waker(ctx.waker().clone());
            Poll::Pending
        }
    }

    #[inline(always)]
    fn event_and_driver(self: Pin<&mut Self>) -> (&mut E, Pin<&mut D>) {
        unsafe {
            let this: &mut Submission<E, D> = Pin::get_unchecked_mut(self);
            (&mut this.event, Pin::new_unchecked(&mut this.driver))
        }
    }
}

impl<E, D> Future for Submission<E, D> where
    E: Event,
    D: Drive,
{
    type Output = (E, io::Result<usize>);

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            match self.state {
                // In the waiting state, first attempt to prepare the event
                // for submission. If that succeeds, also attempt to submit it.
                State::Waiting     => {
                    ready!(self.as_mut().try_prepare(ctx));
                    ready!(self.as_mut().try_submit(ctx));
                    Poll::Pending
                }

                // If the event has been prepared but not submitted, first
                // try to complete it to check if it has been opportunistically
                // opportunistically submitted elsewhere. If not, attempt to
                // submit the event.
                State::Prepared    => {
                    match self.as_mut().try_complete(ctx) {
                        ready @ Poll::Ready(..) => ready,
                        Poll::Pending           => {
                            ready!(self.as_mut().try_submit(ctx));
                            Poll::Pending
                        }
                    }
                }

                // If the event is submitted, there is no work but to try to
                // complete it.
                State::Submitted   => self.try_complete(ctx),

                // If the event is completed, this future has been called after
                // returning ready.
                State::Complete    => panic!("Submission future polled after completion finished"),
            }
        }
    }
}


impl<E: Event, D> Drop for Submission<E, D> {
    fn drop(&mut self) {
        if matches!(self.state, State::Prepared | State::Submitted) {
            unsafe {
                self.completion.as_ref().cancel(Event::cancellation(&mut self.event));
            }
        }
    }
}

#[inline(always)]
unsafe fn prepare<E: Event>(
    sqe: iou::SubmissionQueueEvent<'_>,
    ctx: &mut Context<'_>,
    event: &mut E,
) -> NonNull<Completion> {
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

    let mut sqe = SubmissionCleaner(sqe);
    event.prepare(&mut sqe.0);

    let completion = Box::new(Completion::new(ctx.waker().clone()));
    let completion = NonNull::new_unchecked(Box::into_raw(completion));
    sqe.0.set_user_data(completion.as_ptr() as usize as u64);
    mem::forget(sqe);
    completion
}

