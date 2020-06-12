use std::future::Future;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::{Event, Drive};
use crate::completion::Completion;
use crate::drive::Completion as ExternalCompletion;

/// A [`Future`] representing an event submitted to io-uring
pub struct Submission<E: Event, D> {
    state: State,
    event: ManuallyDrop<E>,
    driver: D,
    completion: Completion,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum State {
    Waiting,
    Prepared,
    Submitted,
    Complete,
    Lost,
}

impl<E: Event, D: Drive> Submission<E, D> {
    /// Construct a new submission from an event and a driver.
    pub fn new(event: E, driver: D) -> Submission<E, D> {
        Submission {
            state: State::Waiting,
            event: ManuallyDrop::new(event),
            completion: Completion::dangling(),
            driver,
        }
    }

    /// Access the driver this submission is using
    pub fn driver(&self) -> &D {
        &self.driver
    }

    #[inline(always)]
    unsafe fn try_prepare(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        let this = Pin::get_unchecked_mut(self);
        let driver = Pin::new_unchecked(&mut this.driver);
        let event = &mut *this.event;
        let state = &mut this.state;
        let completion = ready!(driver.poll_prepare(ctx, |sqe, ctx| {
            prepare(sqe, ctx, event, state)
        }));
        *state = State::Prepared;
        this.completion = completion.real;
        Poll::Ready(())
    }

    #[inline(always)]
    unsafe fn try_submit(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        let (event, driver) = self.as_mut().event_and_driver();
        // TODO figure out how to handle this result
        let _ = ready!(driver.poll_submit(ctx, event.is_eager()));
        Pin::get_unchecked_mut(self).state = State::Submitted;
        Poll::Ready(())
    }

    #[inline(always)]
    unsafe fn try_complete(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<(E, io::Result<usize>)> {
        let this = Pin::get_unchecked_mut(self);
        if let Some(result) = this.completion.check() {
            this.state = State::Complete;
            this.completion.deallocate();
            let event = ManuallyDrop::take(&mut this.event);
            Poll::Ready((event, result))
        } else {
            this.completion.set_waker(ctx.waker().clone());
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
                State::Complete     => panic!("Submission future polled after completion finished"),

                // The "Lost" state indicates that the event was prepared, but
                // its completion was not saved. This can only occur when the
                // driver is incorrectly implemented: preparing a completion
                // but then not returning it to us.
                State::Lost         => panic!("Submission future in a bad state; driver is faulty"),
            }
        }
    }
}


impl<E: Event, D> Drop for Submission<E, D> {
    fn drop(&mut self) {
        unsafe {
            if matches!(self.state, State::Prepared | State::Submitted) {
                self.completion.cancel(Event::cancellation(&mut self.event));
            } else if self.state == State::Waiting {
                ManuallyDrop::drop(&mut self.event);
            }
        }
    }
}

#[inline(always)]
unsafe fn prepare<'cx, E: Event>(
    sqe: iou::SubmissionQueueEvent<'_>,
    ctx: &mut Context<'cx>,
    event: &mut E,
    state: &mut State,
) -> ExternalCompletion<'cx> {
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
    
    // NB: State is put into the `Lost` state in case the driver fails to fulfill its side of the
    // contract and return the Completion back to us. In the Lost state, future attempts to poll
    // the submission will panic.
    *state = State::Lost;

    let completion = Completion::new(ctx.waker().clone());
    sqe.0.set_user_data(completion.addr());
    mem::forget(sqe);
    ExternalCompletion::new(completion, ctx)
}

