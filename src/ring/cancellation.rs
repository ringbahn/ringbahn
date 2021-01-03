use std::any::Any;
use std::ffi::CString;
use std::io;
use std::mem;
use std::os::unix::io::RawFd;
use std::ptr;

use either::Either;

use crate::buf::Buffer;

/// A cancellation callback to clean up resources when IO gets cancelled.
///
/// When a user cancels interest in an event, the future representing that event needs to be
/// dropped. However, that future may share ownership of some data structures (like buffers)
/// with the kernel, which is completing the event. The cancellation callback will take
/// ownership of those resources, and clean them up when it is dropped.
pub struct Cancellation {
    data: *mut (),
    metadata: usize,
    handler: unsafe fn(*mut (), usize, Option<io::Result<u32>>),
}

pub unsafe trait Cancel {
    fn into_raw(self) -> (*mut (), usize);

    /// The handle() method will be called when in two cases:
    /// - the Cancellation object has been dropped. It means the corresponding io_uring event
    ///   has been completed or the Cancellation object has been discarded. The `result` parameter
    ///   is set to None in this case.
    /// - the cancelled event has been processed by the kernel. The 'result' parameter contains
    ///   the `CQE::result()` returned from the kernel.
    unsafe fn handle(data: *mut (), metadata: usize, result: Option<io::Result<u32>>);
}

unsafe impl<T> Cancel for Box<T> {
    fn into_raw(self) -> (*mut (), usize) {
        (Box::into_raw(self) as *mut (), 0)
    }

    unsafe fn handle(data: *mut (), _metadata: usize, _result: Option<io::Result<u32>>) {
        drop(Box::from_raw(data as *mut T));
    }
}

unsafe impl<T> Cancel for Box<[T]> {
    fn into_raw(self) -> (*mut (), usize) {
        let len = self.len();
        (Box::into_raw(self) as *mut (), len)
    }

    unsafe fn handle(data: *mut (), len: usize, _result: Option<io::Result<u32>>) {
        drop(Vec::from_raw_parts(data as *mut T, len, len).into_boxed_slice());
    }
}

#[repr(C)]
struct TraitObject {
    data: *mut (),
    vtable: *mut (),
}

unsafe impl Cancel for Box<dyn Any + Send + Sync> {
    fn into_raw(self) -> (*mut (), usize) {
        let obj = unsafe { mem::transmute::<Self, TraitObject>(self) };
        (obj.data, obj.vtable as usize)
    }

    unsafe fn handle(data: *mut (), metadata: usize, _result: Option<io::Result<u32>>) {
        let obj = TraitObject {
            data,
            vtable: metadata as *mut (),
        };
        drop(mem::transmute::<TraitObject, Self>(obj));
    }
}

unsafe impl Cancel for iou::registrar::RegisteredBuf {
    fn into_raw(self) -> (*mut (), usize) {
        self.into_inner().into_raw()
    }

    unsafe fn handle(data: *mut (), metadata: usize, result: Option<io::Result<u32>>) {
        Box::<[u8]>::handle(data, metadata, result)
    }
}

unsafe impl Cancel for CString {
    fn into_raw(self) -> (*mut (), usize) {
        (self.into_raw() as *mut (), 0)
    }

    unsafe fn handle(data: *mut (), _metadata: usize, _result: Option<io::Result<u32>>) {
        drop(CString::from_raw(data as *mut libc::c_char));
    }
}

unsafe impl Cancel for () {
    fn into_raw(self) -> (*mut (), usize) {
        (ptr::null_mut(), 0)
    }

    unsafe fn handle(_data: *mut (), _metadata: usize, _result: Option<io::Result<u32>>) {}
}

/// A newtype to cancel a raw file descriptor.
///
/// The `RawFd` type is a little special, which is defined as "type RawFd = c_int". It's unsafe to
/// impl Cancel for RawFd, so introduce a newtype to safely reclaim RawFd on cancellation.
pub struct RawFdCancellation(u64);

impl RawFdCancellation {
    pub fn new(fd: RawFd, pending_close: bool) -> Self {
        let flags = if pending_close { 0x1u64 << 32 } else { 0 };

        RawFdCancellation(fd as u64 | flags)
    }
}

unsafe impl Cancel for RawFdCancellation {
    fn into_raw(self) -> (*mut (), usize) {
        (self.0 as usize as *mut (), 0)
    }

    unsafe fn handle(data: *mut (), _metadata: usize, result: Option<io::Result<u32>>) {
        let data = data as u64;
        let fd = data as RawFd;
        let pending_close = (data & 0x1_0000_0000u64) != 0;

        if pending_close {
            // Special handling for a pending IORING_OP_CLOSE request
            match result {
                // If the event has already completed, discard the cancellation request.
                None => {}
                // If the event succeeds, the file descriptor has already been closed.
                Some(Ok(_)) => {}
                // If the event fails, the file descriptor has not been closed yet.
                Some(Err(_)) => {
                    let _ = libc::close(fd);
                }
            }
        } else {
            libc::close(fd);
        }
    }
}

pub unsafe trait CancelNarrow: Cancel {}

unsafe impl<T> CancelNarrow for Box<T> {}
unsafe impl CancelNarrow for CString {}
unsafe impl CancelNarrow for RawFdCancellation {}

unsafe impl<T: CancelNarrow, U: CancelNarrow> Cancel for (T, U) {
    fn into_raw(self) -> (*mut (), usize) {
        let left = self.0.into_raw().0;
        let right = self.1.into_raw().0;
        (left, right as usize)
    }

    unsafe fn handle(data: *mut (), metadata: usize, result: Option<io::Result<u32>>) {
        let res = match result.as_ref() {
            Some(Ok(v)) => Some(Ok(*v)),
            Some(Err(e)) => Some(Err(io::Error::from_raw_os_error(
                e.raw_os_error().unwrap_or(libc::EINVAL),
            ))),
            None => None,
        };

        T::handle(data, 0, result);
        U::handle(metadata as *mut (), 0, res);
    }
}

impl Cancellation {
    fn new<T: Cancel>(object: T) -> Cancellation {
        let (data, metadata) = object.into_raw();
        Cancellation {
            data,
            metadata,
            handler: T::handle,
        }
    }

    pub fn handle(self, result: io::Result<u32>) {
        unsafe { (self.handler)(self.data, self.metadata, Some(result)) }
    }
}

impl<T: Cancel> From<T> for Cancellation {
    fn from(object: T) -> Cancellation {
        Cancellation::new(object)
    }
}

impl<T> From<Option<T>> for Cancellation
where
    Cancellation: From<T>,
{
    fn from(object: Option<T>) -> Cancellation {
        object.map_or(Cancellation::new(()), Cancellation::from)
    }
}

impl From<Buffer> for Cancellation {
    fn from(buffer: Buffer) -> Cancellation {
        Cancellation::from(buffer.into_boxed_slice())
    }
}

impl<T, U> From<Either<T, U>> for Cancellation
where
    Cancellation: From<T> + From<U>,
{
    fn from(object: Either<T, U>) -> Cancellation {
        object.either(Cancellation::from, Cancellation::from)
    }
}

unsafe impl Send for Cancellation {}
unsafe impl Sync for Cancellation {}

impl Drop for Cancellation {
    fn drop(&mut self) {
        unsafe { (self.handler)(self.data, self.metadata, None) }
    }
}
