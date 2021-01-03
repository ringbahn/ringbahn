use iou::sqe::BufferGroupId;
use std::mem::ManuallyDrop;

use super::{Cancellation, Event, SQE};

pub struct ProvideBuffers {
    pub bufs: Box<[u8]>,
    pub count: u32,
    pub group: BufferGroupId,
    pub index: u32,
}

impl Event for ProvideBuffers {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_provide_buffers(&mut self.bufs[..], self.count, self.group, self.index);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).bufs)
    }
}

pub struct RemoveBuffers {
    pub count: u32,
    pub group: BufferGroupId,
}

impl Event for RemoveBuffers {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_remove_buffers(self.count, self.group);
    }
}
