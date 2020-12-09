use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::sqe::{MsgFlags, SockAddr};
use iou::registrar::UringFd;

use super::{Event, SQE, SQEs, Cancellation};

/// A socket `sendto` event.
pub struct SendTo<FD: UringFd = RawFd> {
    fd: FD,
    buf: Box<[u8]>,
    addr: SockAddr,
    flags: MsgFlags,
    mhdr: libc::msghdr,
}

impl<FD: UringFd> SendTo<FD> {
    pub fn new(fd: FD, buf: Box<[u8]>, addr: SockAddr, flags: MsgFlags) -> SendTo<FD> {
        SendTo { fd, buf, addr, flags, mhdr: unsafe { std::mem::zeroed() } }
    }
}

impl<FD: UringFd + Copy> Event for SendTo<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        // Safety: *const _ as *mut _ is safe here because the resulting pointers
        // are required to be *mut by C but are never written to.
        let (name, len) = self.addr.as_ffi_pair();
        self.mhdr.msg_name = name as *const _ as *mut _;
        self.mhdr.msg_namelen = len;
        self.mhdr.msg_iov = &self.buf as *const _ as *const std::io::IoSlice as *mut libc::iovec;
        self.mhdr.msg_iovlen = 1;

        let mut sqe = sqs.single().unwrap();
        sqe.prep_sendmsg(self.fd, &mut self.mhdr as *mut _, self.flags);
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}
