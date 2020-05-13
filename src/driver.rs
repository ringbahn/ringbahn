use std::io;
use std::pin::Pin;
use std::ptr::NonNull;
use std::thread;
use std::task::{Context, Poll};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::{Submit, Completion};

const ENTRIES: u32 = 32;

pub static DRIVER: Driver = Driver {
    driver: Lazy::new(init)
};

pub struct Driver {
    driver: Lazy<Mutex<iou::SubmissionQueue<'static>>>,
}

impl<'a> Submit for &'a Driver {
    fn poll_prepare(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'_>) -> NonNull<Completion>,
    ) -> Poll<NonNull<Completion>> {
        let mut driver = self.driver.lock();
        loop {
            match driver.next_sqe() {
                Some(sqe)   => return Poll::Ready(prepare(sqe, ctx)),
                None        => { let _ = driver.submit(); }
            }
        }
    }

    fn poll_submit(self: Pin<&mut Self>, _: &mut Context<'_>, eager: bool) -> Poll<io::Result<usize>> {
        let result = if eager {
            self.driver.lock().submit()
        } else {
            Ok(0)
        };
        Poll::Ready(result)
    }
}

fn init() -> Mutex<iou::SubmissionQueue<'static>> {
    unsafe {
        static mut RING: Option<iou::IoUring> = None;
        RING = Some(iou::IoUring::new(ENTRIES).expect("TODO handle io_uring_init failure"));
        let (sq, cq, _) = RING.as_mut().unwrap().queues();
        thread::spawn(move || complete(cq));
        Mutex::new(sq)
    }
}

unsafe fn complete(mut cq: iou::CompletionQueue<'static>) {
    // TODO handle IO errors on completion returning
    while let Ok(cqe) = cq.wait_for_cqe() {
        crate::complete(cqe);
        while let Some(cqe) = cq.peek_for_cqe() {
            crate::complete(cqe);
        }
    }
}
