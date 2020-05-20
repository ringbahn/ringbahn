mod completion;

pub mod demo;

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub use completion::{Completion, complete};

use crate::{Submission, Event};

pub trait Drive {
    fn poll_prepare(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'_>) -> Completion,
    ) -> Poll<Completion>;

    fn poll_submit(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        eager: bool,
    ) -> Poll<io::Result<usize>>;

    fn submit<E: Event>(self, event: E) -> Submission<E, Self> where Self: Sized {
        Submission::new(event, self)
    }
}
