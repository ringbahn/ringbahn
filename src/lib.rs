mod cancellation;
mod completion;
mod submission;

mod driver;
pub mod event;

use std::io;
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Context, Poll};

pub use cancellation::Cancellation;
pub use completion::{Completion, complete};
pub use submission::Submission;

pub use event::Event;
pub use driver::{DRIVER, Driver};

pub trait Submit {
    fn poll_prepare(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'_>) -> NonNull<Completion>,
    ) -> Poll<NonNull<Completion>>;

    fn poll_submit(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        eager: bool,
    ) -> Poll<io::Result<usize>>;

    fn submit<E: Event>(self, event: E) -> Submission<E, Self> where Self: Sized {
        Submission::new(event, self)
    }
}
