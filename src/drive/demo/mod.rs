mod access_queue;

use std::cell::UnsafeCell;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::Once;
use std::task::{Poll, Context};
use std::thread;

use futures_core::ready;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

const ENTRIES: u32 = 512;

use access_queue::*;

use super::{Drive, Completion};

pub fn driver() -> DemoDriver<'static> {
    unsafe {
        static QUEUES: Lazy<AccessQueue<Queues<'static>>> = Lazy::new(init);
        static ONCE: Once = Once::new();

        ONCE.call_once(|| {
            thread::spawn(|| complete(&QUEUES));
        });
        DemoDriver::new(&QUEUES)
    }
}

impl Default for DemoDriver<'static> {
    fn default() -> DemoDriver<'static> {
        driver()
    }
}

pub struct DemoDriver<'a> {
    queues: WillAccess<'a, Queues<'a>>,
}

impl<'a> DemoDriver<'a> {
    fn new(queues: &'a AccessQueue<Queues<'a>>) -> DemoDriver<'a> {
        DemoDriver { queues: queues.enqueue() }
    }
}

impl<'a> Drive for DemoDriver<'a> {
    fn poll_prepare(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>, &mut Context<'_>) -> NonNull<Completion>,
    ) -> Poll<NonNull<Completion>> {
        let queues = ready!(Pin::new(&mut self.queues).poll(ctx));

        let mut sq = queues.sq.lock();
        loop {
            match sq.next_sqe() {
                Some(sqe)   => {
                    let completion = prepare(sqe, ctx);
                    drop(sq);
                    // Do not release our access on the queues until the completion
                    // actually completes
                    queues.hold_indefinitely();
                    return Poll::Ready(completion)
                }
                None        => { let _ = sq.submit(); }
            }
        }
    }

    fn poll_submit(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        eager: bool,
    ) -> Poll<io::Result<usize>> {
        if eager {
            Poll::Ready(self.queues.skip_queue().sq.lock().submit())
        } else {
            Poll::Ready(Ok(0))
        }
    }
}

struct Queues<'a> {
    sq: Mutex<iou::SubmissionQueue<'a>>,
    cq: UnsafeCell<iou::CompletionQueue<'a>>,
}

unsafe impl Send for Queues<'_> { }
unsafe impl Sync for Queues<'_> { }

impl<'a> Queues<'a> {
    fn new(ring: &'a mut iou::IoUring) -> Queues<'a> {
        let (sq, cq, ..) = ring.queues();
        Queues { sq: Mutex::new(sq), cq: UnsafeCell::new(cq) }
    }
}


fn init() -> AccessQueue<Queues<'static>> {
    unsafe {
        static mut RING: Option<iou::IoUring> = None;
        RING = Some(iou::IoUring::new(ENTRIES).expect("TODO handle io_uring_init failure"));
        let queues = AccessQueue::new(Queues::new(RING.as_mut().unwrap()), (ENTRIES * 2) as usize);
        queues
    }
}

unsafe fn complete(queues: &AccessQueue<Queues<'_>> ) {
    let cq: &mut iou::CompletionQueue<'_> = &mut *queues.skip_queue().cq.get();
    // TODO handle IO errors on completion returning
    while let Ok(cqe) = cq.wait_for_cqe() {
        let mut lock = queues.lock();
        lock.release(1);
        super::complete(cqe);
        while let Some(cqe) = cq.peek_for_cqe() {
            lock.release(1);
            super::complete(cqe);
        }
    }
}
