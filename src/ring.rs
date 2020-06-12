use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::completion::Completion;
use crate::drive::Completion as ExternalCompletion;
use crate::drive::Drive;
use crate::event::Cancellation;

use State::*;

/// A low-level primitive for building an IO object on io-uring
/// 
/// Ring is a state machine similar to `Submission`, but it is designed to cycle through multiple
/// IO events submitted to io-uring, rather than representing a single submission. Because of this,
/// it is more low level, but it is suitable fro building an IO object like a `File` on top of
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
        prepare: impl FnOnce(&mut iou::SubmissionQueueEvent<'_>),
    ) -> Poll<io::Result<usize>> {
        unsafe {
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
    }

    #[inline(always)]
    unsafe fn try_prepare(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(&mut iou::SubmissionQueueEvent<'_>),
    ) -> Poll<()> {
        let (driver, mut state) = self.as_mut().driver_and_state();
        let completion = ready!(driver.poll_prepare(ctx, |mut sqe, ctx| {
            *state = Lost;
            prepare(&mut sqe);
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
                completion.set_waker(ctx.waker());
            }
            Poll::Pending
        }
    }

    /// Cancel any ongoing IO with this cancellation.
    ///
    /// Users are responsible for ensuring that the cancellation passed would be appropriate to
    /// clean up the resources of the running event.
    #[inline]
    pub fn cancel(&mut self, mut cancellation: Cancellation) {
        unsafe {
            match self.completion.take() {
                Some(completion)    => completion.cancel(cancellation),
                None                => cancellation.cancel(),
            }
        }
    }

    /// Cancel any ongoing IO, but from a pinned reference.
    ///
    /// This has the same behavior of as Ring::cancel.
    pub fn cancel_pinned(self: Pin<&mut Self>, cancellation: Cancellation) {
        unsafe { Pin::get_unchecked_mut(self).cancel(cancellation) }
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
