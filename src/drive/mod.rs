pub mod demo;

use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::completion;
use crate::{Submission, Event};

pub use crate::completion::complete;

pub struct Completion<'cx> {
    pub(crate) real: completion::Completion,
    marker: PhantomData<fn(&'cx ()) -> &'cx ()>,
}

impl<'cx> Completion<'cx> {
    pub(crate) fn new(real: completion::Completion, _: &mut Context<'cx>) -> Completion<'cx> {
        Completion { real, marker: PhantomData }
    }
}

pub trait Drive {
    fn poll_prepare<'cx>(
        self: Pin<&mut Self>,
        ctx: &mut Context<'cx>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'cx>) -> Completion<'cx>,
    ) -> Poll<Completion<'cx>>;

    fn poll_submit(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        eager: bool,
    ) -> Poll<io::Result<usize>>;

    fn submit<E: Event>(self, event: E) -> Submission<E, Self> where Self: Sized {
        Submission::new(event, self)
    }
}
