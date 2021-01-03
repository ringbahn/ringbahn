use std::any::Any;
use std::ffi::CString;
use std::mem;
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
    drop: unsafe fn(*mut (), usize),
}

pub unsafe trait Cancel {
    fn into_raw(self) -> (*mut (), usize);
    unsafe fn drop_raw(data: *mut (), metadata: usize);
}

unsafe impl<T> Cancel for Box<T> {
    fn into_raw(self) -> (*mut (), usize) {
        (Box::into_raw(self) as *mut (), 0)
    }

    unsafe fn drop_raw(data: *mut (), _: usize) {
        drop(Box::from_raw(data as *mut T));
    }
}

unsafe impl<T> Cancel for Box<[T]> {
    fn into_raw(self) -> (*mut (), usize) {
        let len = self.len();
        (Box::into_raw(self) as *mut (), len)
    }

    unsafe fn drop_raw(data: *mut (), len: usize) {
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

    unsafe fn drop_raw(data: *mut (), metadata: usize) {
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

    unsafe fn drop_raw(data: *mut (), metadata: usize) {
        Box::<[u8]>::drop_raw(data, metadata)
    }
}

unsafe impl Cancel for CString {
    fn into_raw(self) -> (*mut (), usize) {
        (self.into_raw() as *mut (), 0)
    }

    unsafe fn drop_raw(data: *mut (), _: usize) {
        drop(CString::from_raw(data as *mut libc::c_char));
    }
}

unsafe impl Cancel for () {
    fn into_raw(self) -> (*mut (), usize) {
        (ptr::null_mut(), 0)
    }

    unsafe fn drop_raw(_: *mut (), _: usize) {}
}

pub unsafe trait CancelNarrow: Cancel {}

unsafe impl<T> CancelNarrow for Box<T> {}
unsafe impl CancelNarrow for CString {}

unsafe impl<T: CancelNarrow, U: CancelNarrow> Cancel for (T, U) {
    fn into_raw(self) -> (*mut (), usize) {
        let left = self.0.into_raw().0;
        let right = self.1.into_raw().0;
        (left, right as usize)
    }

    unsafe fn drop_raw(data: *mut (), metadata: usize) {
        T::drop_raw(data, 0);
        U::drop_raw(metadata as *mut (), 0);
    }
}

impl Cancellation {
    fn new<T: Cancel>(object: T) -> Cancellation {
        let (data, metadata) = object.into_raw();
        Cancellation {
            data,
            metadata,
            drop: T::drop_raw,
        }
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
        unsafe { (self.drop)(self.data, self.metadata) }
    }
}
