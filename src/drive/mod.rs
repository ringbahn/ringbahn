//! Drive IO on io-uring

pub mod demo;

use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::completion;
use crate::{Submission, Event};
use iou::{SQE, SQEs};

pub use completion::complete;

/// A completion which will be used to wake the task waiting on this event.
///
/// This type is opaque to users of ringbahn. It is constructed by the callback passed to
/// [Drive::poll_prepare].
pub struct Completion<'cx> {
    pub(crate) real: completion::Completion,
    marker: PhantomData<fn(&'cx ()) -> &'cx ()>,
}

impl<'cx> Completion<'cx> {
    pub(crate) fn new(mut sqe: SQE<'_>, _sqes: SQEs<'_>, cx: &mut Context<'cx>) -> Completion<'cx> {
        let real = completion::Completion::new(cx.waker().clone());
        unsafe {
            sqe.set_user_data(real.addr());
        }

        Completion { real, marker: PhantomData }
    }
}

/// Implemented by drivers for io-uring.
///
/// The type that implements `Drive` is used to prepare and submit IO events to an io-uring
/// instance. Paired with a piece of code which processes completions, it can run IO on top of
/// io-uring.
pub trait Drive {
    /// Prepare an event on the submission queue.
    ///
    /// The implementer is responsible for provisioning an [`iou::SQE`] from the
    /// submission queue. Once an SQE is available, the implementer should pass it to the
    /// `prepare` callback, which constructs a [`Completion`], and return that `Completion` to the
    /// caller.
    ///
    /// If the driver is not ready to receive more events, it can return `Poll::Pending`. If it
    /// does, it must register a waker to wake the task when more events can be prepared, otherwise
    /// this method will not be called again. This allows the driver to implement backpressure.
    ///
    /// Drivers which call `prepare` but do not return the completion it gives are incorrectly
    /// implemented. This will lead ringbahn to panic.
    fn poll_prepare<'cx>(
        self: Pin<&mut Self>,
        ctx: &mut Context<'cx>,
        count: u32,
        prepare: impl FnOnce(SQEs<'_>, &mut Context<'cx>) -> Completion<'cx>,
    ) -> Poll<Completion<'cx>>;

    /// Suggest to submit all of the events on the submission queue.
    ///
    /// The implementer is responsible for determining how and when events are submitted to the
    /// kernel to complete. It is valid for this function to do nothing at all; this function just
    /// informs the driver that the user program is waiting for its prepared events to be
    /// submitted and completed.
    ///
    /// If the implementation is not ready to submit, but wants to be called again to try later, it
    /// can return `Poll::Pending`. If it does, it must register a waker to wake the task when it
    /// would be appropriate to try submitting again.
    ///
    /// It is also valid not to submit an event but not to register a waker to try again, in which
    /// case the appropriate response would be to return `Ok(0)`. This indicates to the caller that
    /// the submission step is complete, whether or not actual IO was performed by the driver.
    fn poll_submit(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<io::Result<u32>>;

    fn submit<E: Event>(self, event: E) -> Submission<E, Self> where Self: Sized {
        Submission::new(event, self)
    }
}
