use std::io;
use std::mem::{self, ManuallyDrop};
use std::task::Waker;

use parking_lot::Mutex;

use crate::ring::Cancellation;
use iou::CQE;

use State::*;

/// A completion tracks an event that has been submitted to io-uring. It is a pointer to a heap
/// allocated object which represents the state of the event's completion. Ownership of this object
/// is shared between the Completion type and the io-uring instance (the address of the object is
/// passed as a user_data field with the event's SQE).
///
/// Therefore, it requires a fair amount of unsafe code and synchronization to properly manage the
/// lifecycle of this object. That code is encapsulated here inside a safe API for the rest of
/// ringbahn to use.
///
/// This API is not publicly visible outside of this crate. (The Completion type in the public API
/// is an opaque wrapper aroud this type). End users do not need to understand the completion API.
pub struct Completion {
    state: ManuallyDrop<Box<Mutex<State>>>,
}

enum State {
    Submitted(Waker),
    Completed(io::Result<u32>),
    Cancelled(Cancellation),
    Empty,
}

impl Completion {
    /// Create a new completion for an event being prepared. When the event is completed by
    /// io-uring, the waker this completion holds will be awoken.
    pub fn new(waker: Waker) -> Completion {
        Completion {
            state: ManuallyDrop::new(Box::new(Mutex::new(Submitted(waker)))),
        }
    }

    /// Get the address of this completion, so that it can set as the user_data field of the SQE
    /// being prepared.
    pub fn addr(&self) -> u64 {
        &**self.state as *const Mutex<State> as usize as u64
    }

    /// Check if the completion has completed. If it has, the result of the completion will be
    /// returned and the completion will be deallocated. If it has not been completed, the waker
    /// field will be updated to the new waker if the old waker would not wake the same task.
    pub fn check(self, waker: &Waker) -> Result<io::Result<u32>, Completion> {
        let mut state = self.state.lock();
        match mem::replace(&mut *state, State::Empty) {
            Submitted(old_waker) => {
                let waker = if old_waker.will_wake(waker) {
                    old_waker
                } else {
                    waker.clone()
                };
                *state = Submitted(waker);
                drop(state);
                Err(self)
            }
            Completed(result) => {
                drop(state);
                drop(ManuallyDrop::into_inner(self.state));
                Ok(result)
            }
            _ => unreachable!(),
        }
    }

    /// Cancel interest in this completion. The Cancellation callback will be stored to clean up
    /// resources shared with the kernel when the event completes.
    pub fn cancel(self, callback: Cancellation) {
        let mut state = self.state.lock();
        match &*state {
            Submitted(_) => {
                *state = Cancelled(callback);
                drop(state);
            }
            Completed(_) => {
                drop(callback);
                drop(state);
                drop(ManuallyDrop::into_inner(self.state));
            }
            _ => unreachable!(),
        }
    }

    fn complete(self, result: io::Result<u32>) {
        let mut state = self.state.lock();
        match mem::replace(&mut *state, State::Empty) {
            Submitted(waker) => {
                *state = Completed(result);
                waker.wake();
            }
            Cancelled(callback) => {
                callback.handle(result);
                drop(state);
                drop(ManuallyDrop::into_inner(self.state));
            }
            _ => unreachable!(),
        }
    }
}

pub fn complete(cqe: CQE) {
    unsafe {
        let result = cqe.result();
        let user_data = cqe.user_data();
        // iou should never raise LIBURING_UDATA_TIMEOUTs, this is just to catch bugs in iou
        debug_assert!(user_data != uring_sys::LIBURING_UDATA_TIMEOUT);
        let state = user_data as *mut Mutex<State>;

        if !state.is_null() {
            let completion = Completion {
                state: ManuallyDrop::new(Box::from_raw(state)),
            };
            completion.complete(result);
        }
    };
}
