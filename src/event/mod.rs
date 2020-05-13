mod cancellation;

mod read;
mod write;

use std::marker::Unpin;
use std::mem::ManuallyDrop;

use crate::{Submission, Submit};

pub use cancellation::Cancellation;

pub use read::Read;
pub use write::Write;

pub trait Event: Unpin {
    /// Prepare an event to be submitted using this SQE
    ///
    /// ## Safety
    ///
    /// This is an unsafe trait method. It's important to understand that in the case of unsafe
    /// trait methods, the implementer is given *extra guarantees* - it is the caller that is
    /// "unsafe," not the implementer. However, any unsafe operations that are performed inside the
    /// implementation must be sound only in the presence of those additional guarantees and
    /// guarantees the implementer can provide; the implementer cannot assume additional guarantees
    /// beyond those specified here.
    ///
    /// Specifically, when this method is called, these additional guarantees are made:
    ///
    /// No portion of the program will read from or write to this event, or any fields it
    /// holds exclusive, private ownership of, until after the event submitted with this
    /// SQE has been completed by the kernel. Therefore, if any owned data is shared with
    /// the kernel using this SQE, the kernel can be thought of as having "exclusive" ownership oer
    /// that data.
    // FIXME: These docs are not actually true, they gloss over the fact that we do call
    // cancellation and is_eager at certain points. They need to be worded more precisely.
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);

    /// Return the cancellation callback for this event.
    ///
    /// If this event is cancelled, this callback will be stored with the completion to called when
    /// the IO event completes. This way, any managed resources passed to the kernel (like buffers)
    /// can be cleaned up once the kernel no longer needs them.
    fn cancellation(this: &mut ManuallyDrop<Self>) -> Cancellation;

    fn submit<S: Submit>(self, driver: S) -> Submission<Self, S> where Self: Sized {
        Submission::new(self, driver)
    }

    fn is_eager(&self) -> bool {
        true
    }
}
