//! Drive IO on io-uring

pub mod demo;
mod buf;

use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::completion;

pub use crate::completion::complete;
pub use buf::{ProvideBuffer, HeapBuffer};

/// A ccompletion which will be used to wake the task waiting on this event.
///
/// This type is opaque to users of ringbahn. It is constructed by the callback passed to
/// [Drive::poll_prepare].
pub struct Completion<'cx> {
    pub(crate) real: completion::Completion,
    marker: PhantomData<fn(&'cx ()) -> &'cx ()>,
}

impl<'cx> Completion<'cx> {
    pub(crate) fn new(real: completion::Completion, _: &mut Context<'cx>) -> Completion<'cx> {
        Completion { real, marker: PhantomData }
    }
}

/// Implemented by drivers for io-uring.
///
/// The type that implements `Drive` is used to prepare and submit IO events to an io-uring
/// instance. Paired with a piece of code which processes completions, it can run IO on top of
/// io-uring.
pub trait Drive {
    type ReadBuf: ProvideBuffer<Self>;
    type WriteBuf: ProvideBuffer<Self>;

    /// Prepare an event on the submission queue.
    ///
    /// The implementer is responsible for provisioning an [`iou::SubmissionQueueEvent`] from the
    /// submission  queue. Once an SQE is available, the implementer should pass it to the
    /// `prepare` callback, which constructs a [`Completion`], and return that `Completion` to the
    /// caller.
    ///
    /// If the driver is not ready to recieve more events, it can return `Poll::Pending`. If it
    /// does, it must register a waker to wake the task when more events can be prepared, otherwise
    /// this method will not be called again. This allows the driver to implement backpressure.
    ///
    /// Drivers which call `prepare` but do not return the completion it gives are incorrectly
    /// implemented. This will lead ringbahn to panic.
    fn poll_prepare<'cx>(
        self: Pin<&mut Self>,
        ctx: &mut Context<'cx>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'cx>) -> Completion<'cx>,
    ) -> Poll<Completion<'cx>>;

    /// Submit all of the events on the submission queue.
    ///
    /// The implementer is responsible for determining how and when these events are submitted to
    /// the kernel to complete. The `eager` argument is a hint indicating whether the caller would
    /// prefer to see events submitted eagerly, but the implementation is not obligated to follow
    /// this hint.
    ///
    /// If the implementation is not ready to submit, but wants to be called again to try later, it
    /// can return `Poll::Pending`. If it does, it must register a waker to wake the task when it
    /// would be appropriate to try submitting again.
    ///
    /// It is also valid not to submit an event but not to register a waker to try again, in which
    /// case the appropriate response would be to return `Ok(0)`. This indicates to the caller that
    /// the submission step is complete, whether or not actual IO was performed.
    fn poll_submit(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        eager: bool,
    ) -> Poll<io::Result<usize>>;

    fn poll_provide_read_buf(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        capacity: usize,
    ) -> Poll<io::Result<Self::ReadBuf>> {
        Self::ReadBuf::poll_provide(self, ctx, capacity)
    }

    fn poll_provide_write_buf(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        capacity: usize,
    ) -> Poll<io::Result<Self::WriteBuf>> {
        Self::WriteBuf::poll_provide(self, ctx, capacity)
    }
}
