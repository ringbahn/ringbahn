use std::io;
use std::thread;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::Submit;

const ENTRIES: u32 = 32;

pub static DRIVER: Driver = Driver {
    driver: Lazy::new(init)
};

pub struct Driver {
    driver: Lazy<Mutex<iou::SubmissionQueue<'static>>>,
}

impl<'a> Submit for &'a Driver {
    fn prepare(&mut self, prepare: impl FnOnce(iou::SubmissionQueueEvent<'_>)) {
        let mut driver = self.driver.lock();
        loop {
            match driver.next_sqe() {
                Some(sqe)   => break prepare(sqe),
                None        => { let _ = self.submit(); }
            }
        }
    }

    fn submit(&mut self) -> io::Result<usize> {
        self.driver.lock().submit()
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
