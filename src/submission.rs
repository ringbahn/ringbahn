use std::future::Future;
use std::io;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::{ring::Ring, Drive, Event};

/// A [`Future`] representing an event submitted to io-uring
pub struct Submission<E: Event, D: Drive> {
    ring: Ring<D>,
    event: Option<E>,
}

impl<E: Event, D: Drive> Submission<E, D> {
    /// Construct a new submission from an event and a driver.
    pub fn new(event: E, driver: D) -> Submission<E, D> {
        Submission {
            ring: Ring::new(driver),
            event: Some(event),
        }
    }

    /// Access the driver this submission is using
    pub fn driver(&self) -> &D {
        self.ring.driver()
    }

    pub fn replace_event(self: Pin<&mut Self>, event: E) {
        let (ring, event_slot) = self.split();
        if let Some(event) = event_slot.take() {
            ring.cancel_pinned(E::cancel(ManuallyDrop::new(event)))
        }
        *event_slot = Some(event);
    }

    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Option<E>) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.ring), &mut this.event)
        }
    }
}

impl<E, D> Future for Submission<E, D>
where
    E: Event,
    D: Drive,
{
    type Output = (E, io::Result<u32>);

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let (ring, event) = self.split();

        let result = if let Some(event) = event {
            let count = event.sqes_needed();
            ready!(ring.poll(ctx, count, |sqs| unsafe { event.prepare(sqs) }))
        } else {
            panic!("polled Submission after completion")
        };

        Poll::Ready((event.take().unwrap(), result))
    }
}

impl<E: Event, D: Drive> Drop for Submission<E, D> {
    fn drop(&mut self) {
        if let Some(event) = self.event.take() {
            self.ring.cancel(E::cancel(ManuallyDrop::new(event)))
        }
    }
}
