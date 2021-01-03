use std::mem::ManuallyDrop;
use std::time::Duration;

use super::{Cancellation, Event, SQE};

use iou::sqe::TimeoutFlags;

pub struct StaticTimeout {
    ts: uring_sys::__kernel_timespec,
    events: u32,
    flags: TimeoutFlags,
}

impl StaticTimeout {
    pub const fn new(duration: Duration, events: u32, flags: TimeoutFlags) -> StaticTimeout {
        StaticTimeout {
            ts: timespec(duration),
            events,
            flags,
        }
    }
}

impl Event for &'static StaticTimeout {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_timeout(&self.ts, self.events, self.flags);
    }
}

pub struct Timeout {
    ts: Box<uring_sys::__kernel_timespec>,
    events: u32,
    flags: TimeoutFlags,
}

impl Timeout {
    pub fn new(duration: Duration, events: u32, flags: TimeoutFlags) -> Timeout {
        Timeout {
            ts: Box::new(timespec(duration)),
            events,
            flags,
        }
    }
}

impl Event for Timeout {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_timeout(&*self.ts, self.events, self.flags);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).ts)
    }
}

const fn timespec(duration: Duration) -> uring_sys::__kernel_timespec {
    uring_sys::__kernel_timespec {
        tv_sec: duration.as_secs() as i64,
        tv_nsec: duration.subsec_nanos() as _,
    }
}
