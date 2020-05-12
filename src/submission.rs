use std::future::Future;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Context, Poll};

use crate::{Event, Submit, Completion};

pub struct Submission<E: Event, S> {
    event: ManuallyDrop<E>,
    completion: Option<NonNull<Completion>>,
    submitter: Option<S>,
}

impl<E: Event, S: Submit> Submission<E, S> {
    pub fn new(mut event: E, mut submitter: S) -> Submission<E, S> {
        unsafe {
            let completion = Box::new(Completion::new());

            submitter.prepare(|sqe| {
                // Use the SubmissionCleaner guard to clear the submission of any data
                // in case the Event::prepare method panics
                struct SubmissionCleaner<'a>(iou::SubmissionQueueEvent<'a>);

                impl<'a> Drop for SubmissionCleaner<'a> {
                    fn drop(&mut self) {
                        unsafe {
                            self.0.prep_nop();
                            self.0.set_user_data(0);
                        }
                    }
                }

                let mut sqe = SubmissionCleaner(sqe);
                event.prepare(&mut sqe.0);
                sqe.0.set_user_data(&*completion as *const Completion as usize as u64);
                mem::forget(sqe);
            });

            let completion = NonNull::new_unchecked(Box::into_raw(completion));

            Submission {
                event: ManuallyDrop::new(event),
                completion: Some(completion),
                submitter: Some(submitter),
            }
        }
    }
}

impl<E, S> Future for Submission<E, S> where
    E: Event,
    S: Submit,
{
    type Output = (E, io::Result<usize>);

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let this: &mut Submission<E, S> = Pin::get_unchecked_mut(self);
            
            // If we still have a completion, this future is not finished.
            if let Some(completion) = &mut this.completion {
                // Check the completion. If it has completed, this future is ready.
                // Drop the completion and return the event and the result.
                if let Some(result) = completion.as_ref().check() {
                    drop(Box::<Completion>::from_raw(completion.as_ptr()));
                    this.completion = None;
                    let event = ManuallyDrop::take(&mut this.event);
                    Poll::Ready((event, result))
                }

                // Otherwise, check if this future needs to be submitted.
                // If so, set the completion's waker to wake this future
                // and ensure submission.
                else if let Some(mut submitter) = this.submitter.take() {
                    completion.as_mut().set_waker(ctx.waker().clone());
                    // TODO how could we report this error?
                    let _ = submitter.submit();
                    Poll::Pending
                }

                // If not, this was a spurious wake up, just return pending.
                else {
                    Poll::Pending
                }
            } else {
                panic!("Submission future polled after completion finished")
            }
        }
    }
}

impl<E: Event, S> Drop for Submission<E, S> {
    fn drop(&mut self) {
        if let Some(completion) = self.completion {
            unsafe {
                completion.as_ref().cancel(Event::cancellation(&mut self.event))
            }
        }
    }
}
