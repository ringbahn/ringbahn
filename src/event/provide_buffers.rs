use std::mem::ManuallyDrop;

use iou::sqe::BufferGroupId;

use super::{Event, SQE, SQEs, Cancellation};

pub struct ProvideBuffers {
    pub bufs: Box<[u8]>,
    pub count: u32,
    pub group: BufferGroupId,
    pub index: u32,
}

impl Event for ProvideBuffers {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_provide_buffers(&mut self.bufs[..], self.count, self.group, self.index);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        let mut buf: ManuallyDrop<Box<[u8]>> = ManuallyDrop::new(ManuallyDrop::take(this).bufs);
        let cap = buf.len();
        Cancellation::buffer(buf.as_mut_ptr(), cap)
    }
}

pub struct RemoveBuffers {
    pub count: u32,
    pub group: BufferGroupId,
}

impl Event for RemoveBuffers {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_remove_buffers(self.count, self.group);
        sqe
    }

    unsafe fn cancel(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}
