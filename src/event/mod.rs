//! Events that can be scheduled on io-uring with a [`Submission`](crate::Submission)

mod accept;
mod close;
mod connect;
mod epoll_ctl;
mod fadvise;
mod fallocate;
mod files_update;
mod fsync;
mod openat;
mod provide_buffers;
mod read;
mod readv;
mod recv;
mod send;
mod splice;
mod statx;
mod timeout;
mod write;
mod writev;

use std::mem::ManuallyDrop;

use iou::SQE;

use crate::ring::Cancellation;

pub use accept::Accept;
pub use close::Close;
pub use connect::Connect;
pub use epoll_ctl::EpollCtl;
pub use fadvise::Fadvise;
pub use fallocate::Fallocate;
pub use files_update::FilesUpdate;
pub use fsync::Fsync;
pub use openat::OpenAt;
pub use provide_buffers::{ProvideBuffers, RemoveBuffers};
pub use read::{Read, ReadFixed};
pub use readv::ReadVectored;
pub use recv::Recv;
pub use send::Send;
pub use splice::Splice;
pub use statx::Statx;
pub use timeout::{StaticTimeout, Timeout};
pub use write::{Write, WriteFixed};
pub use writev::WriteVectored;

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
    /// In essence implementing prepare, users can write code ass if any heap addresses passed to
    /// the  kernel have passed ownership of that data to the kernel for the time that the event is
    /// completed.
    unsafe fn prepare(&mut self, sqe: &mut SQE);

    /// Return the cancellation callback for this event.
    ///
    /// If this event is cancelled, this callback will be stored with the completion to be dropped
    /// when the IO event completes. This way, any managed resources passed to the kernel (like
    /// buffers) can be cleaned up once the kernel no longer needs them.
    fn cancel(_: ManuallyDrop<Self>) -> Cancellation
    where
        Self: Sized,
    {
        Cancellation::from(())
    }
}
