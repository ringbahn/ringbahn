//! A demo driver for experimentation purposes

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Once;
use std::task::{Poll, Context};
use std::thread;

use futures_core::ready;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

const SQ_ENTRIES: u32   = 32;
const CQ_ENTRIES: usize = (SQ_ENTRIES * 2) as usize;

use access_queue::*;

use super::{Drive, Completion};

use iou::*;

static QUEUES: Lazy<(
    AccessQueue<Mutex<SubmissionQueue<'static>>>,
    Mutex<CompletionQueue<'static>>,
    Registrar<'static>
)> = Lazy::new(init);

/// The driver handle
pub struct DemoDriver<'a> {
    sq: Access<'a, Mutex<SubmissionQueue<'static>>>,
}

impl Default for DemoDriver<'_> {
    fn default() -> Self {
        driver()
    }
}

impl<'a> Clone for DemoDriver<'a> {
    fn clone(&self) -> DemoDriver<'a> {
        driver()
    }
}

impl Drive for DemoDriver<'_> {
    fn poll_prepare<'cx>(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'cx>,
        count: u32,
        prepare: impl FnOnce(SQEs<'_>, &mut Context<'cx>) -> Completion<'cx>,
    ) -> Poll<Completion<'cx>> {
        // Wait for access to prepare. When ready, create a new Access future to wait next time we
        // want to prepare with this driver, and lock the SQ.
        //
        // TODO likely we should be using a nonblocking mutex?
        let access = ready!(Pin::new(&mut self.sq).poll(ctx));
        let (sq, access) = access.hold_and_reenqueue();
        self.sq = access;
        let mut sq = sq.lock();
        loop {
            match sq.prepare_sqes(count) {
                Some(sqs)   => return Poll::Ready(prepare(sqs, ctx)),
                None        => { let _ = sq.submit(); }
            }
        }
    }

    fn poll_submit(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<u32>> {
        start_completion_thread();
        Poll::Ready(self.sq.skip_queue().lock().submit())
    }
}

/// Construct a demo driver handle
pub fn driver() -> DemoDriver<'static> {
    DemoDriver { sq: QUEUES.0.access() }
}

/// Access the registrar
///
/// This will return `None` if events have already been submitted to the driver. The Demo Driver
/// currently only allows registering IO objects prior to submitting IO.
pub fn registrar() -> Option<&'static Registrar<'static>> {
    if STARTED_COMPLETION_THREAD.is_completed() {
        Some(&QUEUES.2)
    } else {
        None
    }

}

fn init() -> (AccessQueue<Mutex<SubmissionQueue<'static>>>, Mutex<CompletionQueue<'static>>, Registrar<'static>) {
    unsafe {
        use std::mem::MaybeUninit;
        static mut RING: MaybeUninit<IoUring> = MaybeUninit::uninit();
        RING = MaybeUninit::new(IoUring::new(SQ_ENTRIES).expect("TODO handle io_uring_init failure"));
        let (sq, cq, reg) = (&mut *RING.as_mut_ptr()).queues();
        (AccessQueue::new(Mutex::new(sq), CQ_ENTRIES), Mutex::new(cq), reg)
    }
}

static STARTED_COMPLETION_THREAD: Once = Once::new();

fn start_completion_thread() {
    STARTED_COMPLETION_THREAD.call_once(|| { thread::spawn(move || {
        let mut cq = QUEUES.1.lock();
        while let Ok(cqe) = cq.wait_for_cqe() {
            let mut ready = cq.ready() as usize + 1;
            QUEUES.0.release(ready);

            super::complete(cqe);
            ready -= 1;

            while let Some(cqe) = cq.peek_for_cqe() {
                if ready == 0 {
                    ready = cq.ready() as usize + 1;
                    QUEUES.0.release(ready);
                }

                super::complete(cqe);
                ready -= 1;
            }

            debug_assert!(ready == 0);
        }
    }); });
}
