pub mod fs;
pub mod drive;
pub mod event;
pub mod net;
pub mod unix;

mod buf;
mod cancellation;
mod completion;
mod ring;
mod submission;

pub use cancellation::Cancellation;
pub use ring::Ring;
pub use submission::Submission;

#[doc(inline)]
pub use drive::Drive;
#[doc(inline)]
pub use event::Event;
#[doc(inline)]
pub use fs::File;

use std::task::Context;

fn finish<'sq, 'cx>(sqe: iou::SQE<'sq>, sqes: iou::SQEs<'sq>, ctx: &mut Context<'cx>) -> drive::Completion<'cx> {
    drive::Completion::new(sqe, sqes, ctx)
}
