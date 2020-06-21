//! Drive IO on io-uring

pub mod demo;
mod buf;

use std::io;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::completion;
use crate::Cancellation;

pub use crate::completion::complete;
pub use buf::{ProvideReadBuf, ProvideWriteBuf, HeapBuffer, RegisteredBuffer, GroupRegisteredBuffer};
pub use buf::BufferId;
pub(crate) use buf::ProvideBufferSealed;

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
    type ReadBuf: ProvideReadBuf<Self>;
    type WriteBuf: ProvideWriteBuf<Self>;

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
        capacity: u32,
    ) -> Poll<io::Result<Self::ReadBuf>> {
        Self::ReadBuf::poll_provide(self, ctx, capacity)
    }

    fn poll_provide_write_buf(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        capacity: u32,
    ) -> Poll<io::Result<Self::WriteBuf>> {
        Self::WriteBuf::poll_provide(self, ctx, capacity)
    }
}

/// A driver which can use pre-registered buffers.
///
/// Buffers provided through this API should be pre-registered with the `io_uring_register`
/// syscall.
pub trait RegisterBuffer: Drive {
    /// Provide a pre-registered buffer.
    ///
    /// This buffer should have been pre-registered with the io-uring instance the drive is a
    /// handle for. The `upper_idx` field of the BufferId struct should be the index of the
    /// preregistered buffer this is a part of. You may use the `lower_idx` field for additional
    /// tracking (for example, if this buffer only represents part of the pre-registered buffer).
    ///
    /// If the buffer returned is *not* part of a pre-registered buffer tracked by the buffer id,
    /// attempts to use this buffer to perform IO will result in errors.
    ///
    /// The capacity argument is just a suggestion: the implementer is allowed to return a heap
    /// buffer with a different capacity from the one requested.
    fn poll_provide_registered(self: Pin<&mut Self>, ctx: &mut Context<'_>, capacity: u32)
        -> Poll<io::Result<(HeapBuffer, BufferId)>>;

    /// Clean up a registered buffer.
    ///
    /// This method will be called when a buffer is no longer in use, though it may be currently
    /// owned by the kernel for some IO event. The constructed cancellation will clean up this
    /// resource as soon as the kernel no longer needs access to the buffer.
    ///
    /// This could deallocate the buffer or it could return the buffer to a pool. The `idx`
    /// argument will be exactly the `idx` passed with this buffer in `poll_provide_registered`,
    /// including the same value in `lower_idx`, which you may use to track custom information
    /// about this buffer.
    fn cleanup_registered(self: Pin<&mut Self>, buf: ManuallyDrop<HeapBuffer>, idx: BufferId) -> Cancellation;
}

/// A driver which can provide buffers from pre-registered buffer groups.
///
/// Buffers provided through this API should be registered with io-uring using the
/// `IORING_OP_PROVIDE_BUFFERS` operation.
pub trait RegisterBufferGroup: Drive {
    /// Provide a group id from which to pull a pre-registered buffer. This will be passed with a
    /// read event, which perform the read into one of the buffers in that group.
    ///
    /// The capacity argument is just a suggestion: the implementer is allowed to return a buffer
    /// group which contains buffers with a different capacity from the one suggested.
    fn poll_provide_group(self: Pin<&mut Self>, ctx: &mut Context<'_>, capacity: u32)
        -> Poll<io::Result<u16>>;

    /// Prepare a cancellation for a request that has used a buffer group.
    ///
    /// When this cancellation's callback is called, the third argument will either be 0, or it
    /// will contain the buffer index the IO was performed into. The buffer with that index in the
    /// group indicated will be have been removed from the kernel's set of available buffers. You
    /// can implement the callback to clean up that buffer in whatever way would be appropriate,
    /// such as deallocating it or returning it to the pool.
    fn cancel_requested_buffer(self: Pin<&mut Self>, group: u16) -> Cancellation;

    /// Access a buffer from the buffer group.
    ///
    /// The BufferId's `upper_idx` field will be the group index, the `lower_idx` field will be the
    /// buffer index within that group.
    ///
    /// # Safety
    ///
    /// This method guarantees that it is called using a group and buffer index returned from a
    /// completed IO event, so it is guaranteed the kernel is not currently accessing it. It is
    /// safe for the caller to return the buffer even though it has previously been shared with the
    /// kernel.
    unsafe fn access_buffer(self: Pin<&mut Self>, idx: BufferId) -> HeapBuffer;

    /// Clean up a buffer that has been taken from a group.
    fn cleanup_group_buffer(self: Pin<&mut Self>, buf: ManuallyDrop<HeapBuffer>, idx: BufferId);
}
