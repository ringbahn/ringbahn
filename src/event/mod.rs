//! Events that can be scheduled on io-uring with a [`Submission`]

mod connect;
mod close;
mod read;
mod readv;
mod openat;
mod write;
mod writev;

use std::mem::ManuallyDrop;

use crate::cancellation::Cancellation;
use crate::kernel::SubmissionSegment;

pub use connect::Connect;
pub use close::Close;
pub use read::Read;
pub use readv::ReadV;
pub use openat::OpenAt;
pub use write::Write;
pub use writev::WriteV;

/// An IO event that can be scheduled on an io-uring driver.
///
/// ## Safety
///
/// Event is a safe trait with two unsafe methods. It's important to understand that when
/// implementing an unsafe method, the code author implementing that method is allowed to assume
/// certain additional invariants will be upheld by all callers. It is the caller's responsibility
/// to ensure those invariants are upheld, not the implementer. However, any unsafe operations
/// performed inside of the method must be safe under those invariants and any other invariants the
/// implementer has upheld. The implementer is not allowed to add any additional invariants that
/// the caller must uphold that are not required by the trait.
pub trait Event {
    fn sqes_needed(&self) -> u32;

    /// Prepare an event to be submitted using the SQE argument.
    ///
    /// ## Safety
    ///
    /// When this method is called, these guarantees will be maintained by the caller:
    ///
    /// The data contained by this event will not be accessed again by this program until one of
    /// two things happen:
    /// - The event being prepared has been completed by the kernel, in which case ownership of
    ///   this event will be passed back to users of this library.
    /// - Interest in the event is cancelled, in which case `Event::cancel` will be called and the
    ///   event's destructor will not run.
    ///
    /// The only method that will be called on this event in the meantime is the `is_eager` method.
    /// Users cannot assume that the is_eager event will not be called.
    ///
    /// In essence implementing prepare, users can write code ass if any heap addresses passed to
    /// the  kernel have passed ownership of that data to the kernel for the time that the event is
    /// completed.
    unsafe fn prepare(&mut self, sqs: &mut SubmissionSegment<'_>);

    /// Return the cancellation callback for this event.
    ///
    /// If this event is cancelled, this callback will be stored with the completion to be dropped
    /// when the IO event completes. This way, any managed resources passed to the kernel (like
    /// buffers) can be cleaned up once the kernel no longer needs them.
    ///
    /// ## Safety
    ///
    /// When this method is called, the event will never accessed again in any way. No methods will
    /// ever be called, including its destructor. The cancellation that is constructed will then
    /// not be dropped until after the event has been completed by the kernel.
    ///
    /// The cancellation can take ownership from the event of any resources owned by the kernel,
    /// and then clean up those resources when the kernel completes the event.
    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation;

    /// Hint if this event is eager.
    fn is_eager(&self) -> bool {
        true
    }
}
