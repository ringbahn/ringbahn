use std::any::Any;
use std::ffi::CString;
use std::mem;
use std::ptr;

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

impl Cancellation {
    pub fn object<T>(object: Box<T>) -> Cancellation {
        unsafe fn callback<T>(object: *mut (), _: usize) {
            drop(Box::from_raw(object as *mut T))
        }
        unsafe { Cancellation::new(Box::into_raw(object) as *mut (), 0, callback::<T>) }
    }

    pub fn dyn_object(object: Box<dyn Any + Send + Sync>) -> Cancellation {
        #[repr(C)] struct TraitObject {
            data: *mut (),
            vtable: *mut (),
        }

        unsafe fn callback(data: *mut (), metadata: usize) {
            let obj = TraitObject { data, vtable: metadata as *mut () };
            drop(mem::transmute::<TraitObject, Box<dyn Any + Send + Sync>>(obj));
        }

        unsafe {
            let obj = mem::transmute::<Box<dyn Any + Send + Sync>, TraitObject>(object);
            Cancellation::new(obj.data, obj.vtable as usize, callback)
        }
    }

    pub fn buffer<T>(buffer: Box<[T]>) -> Cancellation {
        unsafe fn callback<T>(data: *mut (), len: usize) {
            drop::<Box<[T]>>(Vec::<T>::from_raw_parts(data as *mut T, len, len).into_boxed_slice())
        }
        let len = buffer.len();
        unsafe { Cancellation::new(Box::into_raw(buffer) as *mut (), len, callback::<T>) }
    }

    pub fn cstring(cstring: CString) -> Cancellation {
        unsafe fn callback(addr: *mut (), _: usize) {
            drop(CString::from_raw(addr as *mut _));
        }
        unsafe { Cancellation::new(cstring.as_ptr() as *const () as *mut (), 0, callback) }
    }

    /// Construct a new cancellation callback to hold resources until the event concludes. The
    /// `drop` argument will be called receive the `data` and `metadata` fields when the
    /// cancellation is dropped.
    ///
    /// ## Safety
    ///
    /// When this cancellation is dropped, it will call the `drop` closure, which is an unsafe
    /// function. Therefore, whatever invariants are necessary to safely call `drop` (such as
    /// exclusive ownership of some heap allocated data) must met when the cancellation is dropped
    /// as well.
    ///
    /// It must be safe to send the Cancellation type and references to it between threads.
    pub unsafe fn new(data: *mut (), metadata: usize, drop: unsafe fn(*mut (), usize))
        -> Cancellation
    {
        Cancellation { data, metadata, drop }
    }

    /// Construct a null cancellation, which does nothing when it is dropped.
    pub fn null() -> Cancellation {
        unsafe fn drop(_: *mut (), _: usize) { }
        Cancellation { data: ptr::null_mut(), metadata: 0, drop }
    }
}

unsafe impl Send for Cancellation { }
unsafe impl Sync for Cancellation { }

impl Drop for Cancellation {
    fn drop(&mut self) {
        unsafe {
            (self.drop)(self.data, self.metadata)
        }
    }
}
