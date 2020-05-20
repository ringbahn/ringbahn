mod access_queue;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Poll, Context};
use std::thread;

use futures_core::ready;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

const SQ_ENTRIES: u32   = 32;
const CQ_ENTRIES: usize = (SQ_ENTRIES * 2) as usize;

use access_queue::*;

use super::{Drive, Completion};

static SQ:   Lazy<Mutex<iou::SubmissionQueue<'static>>> = Lazy::new(init_sq);
static LOCK: Lazy<AccessController>                     = Lazy::new(init_lock);

pub struct DemoDriver<'a> {
    sq: &'a Lazy<Mutex<iou::SubmissionQueue<'static>>>,
    access: Access<'a>,
}

impl Default for DemoDriver<'_> {
    fn default() -> Self {
        driver()
    }
}

impl<'a> Drive for DemoDriver<'a> {
    fn poll_prepare(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'_>) -> NonNull<Completion>,
    ) -> Poll<NonNull<Completion>> {
        ready!(Pin::new(&mut self.access).poll(ctx));

        let mut sq = self.sq.lock();
        loop {
            match sq.next_sqe() {
                Some(sqe)   => return Poll::Ready(prepare(sqe, ctx)),
                None        => { let _ = sq.submit(); }
            }
        }
    }

    fn poll_submit(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        eager: bool,
    ) -> Poll<io::Result<usize>> {
        let result = if eager {
            self.sq.lock().submit()
        } else {
            Ok(0)
        };
        Poll::Ready(result)
    }
}

pub fn driver() -> DemoDriver<'static> {
    DemoDriver {
        sq: &SQ,
        access: LOCK.access(),
    }
}

fn init_sq() -> Mutex<iou::SubmissionQueue<'static>> {
    unsafe {
        static mut RING: Option<iou::IoUring> = None;
        RING = Some(iou::IoUring::new(SQ_ENTRIES).expect("TODO handle io_uring_init failure"));
        let (sq, cq, _) = RING.as_mut().unwrap().queues();
        thread::spawn(move || complete(cq));
        Mutex::new(sq)
    }
}

fn init_lock() -> AccessController {
    AccessController::new(CQ_ENTRIES)
}

unsafe fn complete(mut cq: iou::CompletionQueue<'static>) {
    while let Ok(cqe) = cq.wait_for_cqe() {
        LOCK.release(cq.ready() as usize + 1);
        super::complete(cqe);
        while let Some(cqe) = cq.peek_for_cqe() {
            super::complete(cqe);
        }
    }
}
