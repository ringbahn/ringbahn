use std::mem::ManuallyDrop;
use std::time::Duration;

use super::{Event, SQE, SQEs, Cancellation};

pub struct StaticTimeout {
    ts: uring_sys::__kernel_timespec,
    events: u32,
}

impl StaticTimeout {
    pub const fn new(duration: Duration, events: u32) -> StaticTimeout {
        StaticTimeout {
            ts: timespec(duration),
            events,
        }
    }
}

impl Event for &'static StaticTimeout {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_timeout(&self.ts, self.events);
        sqe
    }

    unsafe fn cancel(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}

pub struct Timeout {
    ts: Box<uring_sys::__kernel_timespec>,
    events: u32,
}

impl Timeout {
    pub fn new(duration: Duration, events: u32) -> Timeout {
        Timeout {
            ts: Box::new(timespec(duration)),
            events,
        }
    }
}

impl Event for Timeout {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_timeout(&*self.ts, self.events);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn callback(ts: *mut (), _: usize) {
            drop(Box::from_raw(ts as *mut uring_sys::__kernel_timespec))
        }
        Cancellation::new(&mut *this.ts as *mut uring_sys::__kernel_timespec as *mut (), 0, callback)
    }
}

const fn timespec(duration: Duration) -> uring_sys::__kernel_timespec {
    uring_sys::__kernel_timespec {
        tv_sec: duration.as_secs() as i64,
        tv_nsec: duration.subsec_nanos() as _,
    }
}
