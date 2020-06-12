use std::cell::UnsafeCell;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::ptr::{self, NonNull};
use std::task::Waker;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;

use crate::event::Cancellation;

pub struct Completion {
    state: NonNull<State>,
}

unsafe impl Send for Completion { }
unsafe impl Sync for Completion { }

const SUBMITTED: usize = 0;
const COMPLETED: usize = 1;
const CANCELLED: usize = 2;

const UPDATING: usize = 3;

struct State {
    tag: AtomicUsize,
    data: UnsafeCell<Data>,
}

union Data {
    waker: ManuallyDrop<Waker>,
    result: i32,
    callback: ManuallyDrop<Cancellation>,
}

impl Completion {
    pub fn new(waker: Waker) -> Completion {
        unsafe {
            Completion {
                state: NonNull::new_unchecked(Box::into_raw(Box::new(State {
                    tag: AtomicUsize::new(SUBMITTED),
                    data: UnsafeCell::new(Data { waker: ManuallyDrop::new(waker) }),
                }))),
            }
        }
    }

    pub fn dangling() -> Completion {
        Completion {
            state: NonNull::dangling(),
        }
    }

    pub unsafe fn deallocate(self) {
        drop(Box::from_raw(self.state.as_ptr()))
    }

    pub fn addr(&self) -> u64 {
        self.state.as_ptr() as usize as u64
    }

    pub unsafe fn check(&self) -> Option<io::Result<usize>> {
        let state: &State = self.state.as_ref();
        if state.tag.load(SeqCst) == COMPLETED {
            let result = (*state.data.get()).result;
            match result >= 0 {
                true    => Some(Ok(result as usize)),
                false   => Some(Err(io::Error::from_raw_os_error(-result))),
            }
        } else {
            None
        }
    }

    pub unsafe fn set_waker(&self, waker: Waker) {
        let state: &State = self.state.as_ref();
        if state.tag.compare_and_swap(SUBMITTED, UPDATING, SeqCst) == SUBMITTED {
            let old_waker = &mut (*state.data.get()).waker;
            ManuallyDrop::drop(old_waker);
            *old_waker = ManuallyDrop::new(waker);
            state.tag.store(SUBMITTED, SeqCst);
        }
    }

    pub unsafe fn cancel(self, mut callback: Cancellation) {
        let state: &State = self.state.as_ref();
        loop {
            match state.tag.compare_and_swap(SUBMITTED, UPDATING, SeqCst) {
                UPDATING                    => continue,
                SUBMITTED                   => {
                    (*state.data.get()).callback = ManuallyDrop::new(callback);
                    state.tag.store(CANCELLED, SeqCst);
                    break;
                }
                COMPLETED                   => {
                    drop(state);
                    callback.cancel();
                    self.deallocate();
                    break;
                }
                _                           => {
                    debug_assert!(false);
                    break;
                }
            }
        }
    }

    unsafe fn complete(self, result: i32) {
        let state: &State = self.state.as_ref();
        loop {
            match state.tag.compare_and_swap(SUBMITTED, UPDATING, SeqCst) {
                UPDATING                    => continue,
                SUBMITTED                   => {
                    let data: Data = mem::replace(&mut *state.data.get(), Data { result });
                    ManuallyDrop::into_inner(data.waker).wake();
                    state.tag.store(COMPLETED, SeqCst);
                    break;
                }
                CANCELLED                   => {
                    let mut data: Data = mem::replace(&mut *state.data.get(), Data { result });
                    drop(state);
                    data.callback.cancel();
                    self.deallocate();
                    break;
                }
                _                           => {
                    debug_assert!(false);
                    break;
                }
            }
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            match self.tag.load(SeqCst) {
                SUBMITTED   => ManuallyDrop::drop(&mut (*self.data.get()).waker),
                CANCELLED   => ManuallyDrop::drop(&mut (*self.data.get()).callback),
                COMPLETED   => return,
                _           => debug_assert!(false),
            }
        }
    }
}

/// Complete an `[iou::CompletionQueueEvent]` which was constructed from a [`Completion`].
///
/// This function should be used in combination with a driver that implements [`Drive`] to process
/// events on an io-uring instance. This function takes a CQE and processes it.
///
/// ## Safety
///
/// This function is only valid if the user_data in the CQE is null, the liburing timeout
/// signifier, or a pointer to a Completion constructed using ringbahn. If you have scheduled any
/// events on the io-uring instance using a library other than ringbahn, this method is not safe to
/// call unless you have filtered those events out in some manner.
pub unsafe fn complete(cqe: iou::CompletionQueueEvent) {
    if cqe.is_timeout() { return; }

    let completion = cqe.user_data() as *mut State;

    if completion != ptr::null_mut() {
        let completion = Completion { state: NonNull::new_unchecked(completion) };
        completion.complete(cqe.raw_result());
    }
}
