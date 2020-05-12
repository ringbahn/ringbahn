use std::ptr;

pub struct Cancellation {
    data: *mut (),
    metadata: usize,
    callback: unsafe fn(*mut (), usize),
}

impl Cancellation {
    /// Construct a new cancellation callback to be called.
    ///
    /// ## Safety
    ///
    /// The callback argument is an unsafe function pointer. In this case, the
    /// callback can assume these additional invariants:
    ///
    /// The arguments passed into the callback will be the same values as the arguments
    /// passed to Cancellation::new. The callback will be called exactly one or zero
    /// times. The callback will only be called after the kernel has yielded the CQE
    /// associated with the event this callback is meant to cancel.
    pub fn new(data: *mut (), metadata: usize, callback: unsafe fn(*mut (), usize))
        -> Cancellation
    {
        Cancellation { data, metadata, callback }
    }

    pub fn null() -> Cancellation {
        unsafe fn callback(_: *mut (), _: usize) { }
        Cancellation { data: ptr::null_mut(), metadata: 0, callback }
    }

    pub(crate) unsafe fn cancel(&mut self) {
        (self.callback)(self.data, self.metadata)
    }

    pub(crate) unsafe fn buffer(data: *mut u8, len: usize) -> Cancellation {
        unsafe fn callback(data: *mut (), len: usize) {
            drop(Vec::from_raw_parts(data as *mut u8, len, len))
        }

        Cancellation::new(data as *mut (), len, callback)
    }
}
