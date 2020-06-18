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
    /// Construct a new cancellation callback to hold resources until the event concludes. The
    /// `drop` argument will be called recieve the `data` and `metadata` fields when the
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


    pub(crate) unsafe fn buffer(data: *mut u8, len: usize) -> Cancellation {
        unsafe fn drop(data: *mut (), len: usize) {
            std::mem::drop(Vec::from_raw_parts(data as *mut u8, len, len))
        }

        Cancellation::new(data as *mut (), len, drop)
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
