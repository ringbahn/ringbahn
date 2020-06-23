use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::completion::Completion;
use crate::drive::Drive;
use crate::Cancellation;
use crate::kernel::SubmissionSegment;

use State::*;

/// A low-level primitive for building an IO object on io-uring
/// 
/// Ring is a state machine similar to `Submission`, but it is designed to cycle through multiple
/// IO events submitted to io-uring, rather than representing a single submission. Because of this,
/// it is more low level, but it is suitable for building an IO object like a `File` on top of
/// io-uring.
///
/// Users writing code on top of `Ring` are responsible for making sure that it is correct. For
/// example, when calling `poll`, users must ensure that they are in the proper state to submit
/// whatever type of IO they would be attempting to submit. Additionally, users should note that
/// `Ring` does not implement `Drop`. In order to cancel any ongoing IO, users are responsible for
/// implementing drop to call cancel properly.
pub struct Ring<D: Drive> {
    state: State,
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

impl<D: Default + Drive> Default for Ring<D> {
    fn default() -> Ring<D> {
        Ring::new(D::default())
    }
}

impl<D: Drive + Clone> Clone for Ring<D> {
    fn clone(&self) -> Ring<D> {
        Ring::new(self.driver.clone())
    }
}

impl<D: Drive> Ring<D> {
    /// Construct a new Ring on top of a driver.
    #[inline(always)]
    pub fn new(driver: D) -> Ring<D> {
        Ring {
            state: Inert,
            completion: None,
            driver
        }
    }

    pub fn driver(&self) -> &D {
        &self.driver
    }

    /// Poll the ring state machine.
    ///
    /// This accepts a callback, `prepare`, which prepares an event to be submitted to io-uring.
    /// This callback will only be called once during an iteration of ring's state machine: once an
    /// event has been prepared, until it is completed or cancelled, a single ring instance will
    /// not prepare any additional events.
    #[inline]
    pub fn poll(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        is_eager: bool,
        count: u32,
        prepare: impl FnOnce(&mut SubmissionSegment<'_>),
    ) -> Poll<io::Result<u32>> {
        match self.state {
            Inert       => {
                ready!(self.as_mut().poll_prepare(ctx, count, prepare));
                ready!(self.as_mut().poll_submit(ctx, is_eager));
                Poll::Pending
            }
            Prepared    => {
                match self.as_mut().poll_complete(ctx) {
                    ready @ Poll::Ready(..) => ready,
                    Poll::Pending           => {
                        ready!(self.poll_submit(ctx, is_eager));
                        Poll::Pending
                    }
                }
            }
            Submitted   => self.poll_complete(ctx),
            Lost        => panic!("Ring in a bad state; driver is faulty"),
        }
    }

    #[inline(always)]
    fn poll_prepare(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        count: u32,
        prepare: impl FnOnce(&mut SubmissionSegment),
    ) -> Poll<()> {
        let (driver, state, completion_slot) = self.split();
        let completion = ready!(driver.poll_prepare(ctx, count, |mut sqs, ctx| {
            *state = Lost;
            prepare(&mut sqs);
            sqs.finish(ctx)
        }));
        *state = Prepared;
        *completion_slot = Some(completion.real);
        Poll::Ready(())
    }

    #[inline(always)]
    fn poll_submit(self: Pin<&mut Self>, ctx: &mut Context<'_>, is_eager: bool) -> Poll<()> {
        let (driver, state, _) = self.split();
        // TODO figure out how to handle this result
        let _ = ready!(driver.poll_submit(ctx, is_eager));
        *state = Submitted;
        Poll::Ready(())
    }

    #[inline(always)]
    fn poll_complete(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<u32>> {
        let (_, state, completion_slot) = self.split();
        match completion_slot.take().unwrap().check(ctx.waker()) {
            Ok(result)      => {
                *state = Inert;
                Poll::Ready(result)
            }
            Err(completion) => {
                *completion_slot = Some(completion);
                Poll::Pending
            }
        }
    }

    /// Cancel any ongoing IO with this cancellation.
    ///
    /// Users are responsible for ensuring that the cancellation passed would be appropriate to
    /// clean up the resources of the running event.
    #[inline]
    pub fn cancel(&mut self, cancellation: Cancellation) {
        if let Some(completion) = self.completion.take() {
            completion.cancel(cancellation);
        }
    }

    /// Cancel any ongoing IO, but from a pinned reference.
    ///
    /// This has the same behavior of as Ring::cancel.
    pub fn cancel_pinned(self: Pin<&mut Self>, cancellation: Cancellation) {
        unsafe { Pin::get_unchecked_mut(self).cancel(cancellation) }
    }

    fn split(self: Pin<&mut Self>) -> (Pin<&mut D>, &mut State, &mut Option<Completion>) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.driver), &mut this.state, &mut this.completion)
        }
    }
}
