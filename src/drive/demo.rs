//! A demo driver for experimentation purposes

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};
use std::thread;

use event_listener::*;
use futures_core::ready;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

const ENTRIES: u32 = 32;

use super::{Completion, Drive};

use iou::*;

type Queues = (
    Mutex<SubmissionQueue<'static>>,
    Mutex<CompletionQueue<'static>>,
    Registrar<'static>,
    Event,
);

static QUEUES: Lazy<Queues> = Lazy::new(init);

/// The driver handle
pub struct DemoDriver {
    listener: Option<EventListener>,
}

impl DemoDriver {
    fn poll_submit_inner(
        &mut self,
        ctx: &mut Context<'_>,
        sq: &mut SubmissionQueue<'_>,
    ) -> Poll<io::Result<u32>> {
        start_completion_thread();

        loop {
            if let Some(listener) = &mut self.listener {
                ready!(Pin::new(listener).poll(ctx));
            }

            match sq.submit() {
                Ok(n) => {
                    self.listener = None;
                    return Poll::Ready(Ok(n));
                }
                Err(err) => {
                    if err.raw_os_error().map_or(false, |code| code == libc::EBUSY) {
                        if self.listener.is_none() {
                            self.listener = Some(QUEUES.3.listen());
                        }
                    } else {
                        return Poll::Ready(Err(err));
                    }
                }
            }
        }
    }
}

impl Default for DemoDriver {
    fn default() -> Self {
        driver()
    }
}

impl Clone for DemoDriver {
    fn clone(&self) -> DemoDriver {
        driver()
    }
}

impl Drive for DemoDriver {
    fn poll_prepare<'cx>(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'cx>,
        count: u32,
        prepare: impl FnOnce(SQEs<'_>, &mut Context<'cx>) -> Completion<'cx>,
    ) -> Poll<Completion<'cx>> {
        let mut sq = QUEUES.0.lock();
        loop {
            match sq.prepare_sqes(count) {
                Some(sqs) => return Poll::Ready(prepare(sqs, ctx)),
                None => {
                    let _ = ready!(self.poll_submit_inner(ctx, &mut *sq));
                }
            }
        }
    }

    fn poll_submit(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<u32>> {
        let mut guard = QUEUES.0.lock();
        self.poll_submit_inner(ctx, &mut *guard)
    }
}

/// Construct a demo driver handle
pub fn driver() -> DemoDriver {
    DemoDriver { listener: None }
}

/// Access the registrar
///
/// This will return `None` if events have already been submitted to the driver. The Demo Driver
/// currently only allows registering IO objects prior to submitting IO.
pub fn registrar() -> Option<&'static Registrar<'static>> {
    if !STARTED_COMPLETION_THREAD.is_completed() {
        Some(&QUEUES.2)
    } else {
        None
    }
}

fn init() -> Queues {
    let flags = SetupFlags::empty();
    let features = SetupFeatures::NODROP;
    let ring = Box::new(IoUring::new_with_flags(ENTRIES, flags, features).unwrap());
    let ring = Box::leak(ring);
    let (sq, cq, reg) = ring.queues();
    (Mutex::new(sq), Mutex::new(cq), reg, Event::new())
}

static STARTED_COMPLETION_THREAD: Once = Once::new();

fn start_completion_thread() {
    STARTED_COMPLETION_THREAD.call_once(|| {
        thread::spawn(move || {
            let mut cq = QUEUES.1.lock();
            while let Ok(cqe) = cq.wait_for_cqe() {
                let mut ready = cq.ready() as usize + 1;
                QUEUES.3.notify_additional(ready);

                super::complete(cqe);
                ready -= 1;

                while let Some(cqe) = cq.peek_for_cqe() {
                    if ready == 0 {
                        ready = cq.ready() as usize + 1;
                        QUEUES.3.notify_additional(ready);
                    }

                    super::complete(cqe);
                    ready -= 1;
                }

                debug_assert!(ready == 0);
            }
        });
    });
}
