use std::cell::UnsafeCell;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::ptr::{self, NonNull};
use std::sync::atomic::{self, AtomicUsize};
use std::sync::atomic::Ordering::{Acquire, Release};
use std::task::Waker;
use std::thread;

use crate::Cancellation;

/// A completion tracks an event that has been submitted to io-uring. It is a pointer to a heap
/// allocated object which represents the state of the event's completion. Ownership of this object
/// is shared between the Completion type and the io-uring instance (the address of the object is
/// passed as a user_data field with the event's SQE).
///
/// Therefore, it requires a fair amout of unsafe code and synchronization to properly manage the
/// lifecycle of this object. That code is encapsulated here inside a safe API for the rest of
/// ringbahn to use.
///
/// This API is not publicly visible outside of this crate. (The Completion type in the public API
/// is an opaque wrapper aroud this type). End users do not need to understand the completion API.
pub struct Completion {
    state: NonNull<State>,
}

// State is like an enum type, but with an atomic tag field that is also used for synchronization.
//
// When not mid-update, it can be in 3 states:
// 
// state     | tag value | data field | represents
// ----------+-----------+------------+-------------------------------------------------
// Submitted | SUBMITTED | waker      | waiting for a submitted event to complete
// Completed | COMPLETED | result     | event has been completed by io-uring
// Cancelled | CANCELLED | callback   | interest in uncompleted event has been cancelled
//
// Note that once the state is Completed or Cancelled, it is no longer shared ownership: either the
// waiting future or the io-uring will never access it again at that point. Therefore, our CAS lock
// does not actually need to lock if it is in either of those states, only if it is in the
// Submitted state.
struct State {
    tag: AtomicUsize,
    data: UnsafeCell<Data>,
}

const SUBMITTED: usize = 0;
const COMPLETED: usize = 1;
const CANCELLED: usize = 2;
const UPDATING:  usize = 3;

union Data {
    waker: ManuallyDrop<Waker>,
    result: i32,
    callback: ManuallyDrop<Cancellation>,
}

impl Completion {
    /// Create a new completion for an event being prepared. When the event is completed by
    /// io-uring, the waker this completion holds will be awoken.
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

    /// Get the address of this completion, so that it can set as the user_data field of the SQE
    /// being prepared.
    pub fn addr(&self) -> u64 {
        self.state.as_ptr() as usize as u64
    }

    /// Check if the completion has completed. If it has, the result of the completion will be
    /// returned and the completion will be deallocated. If it has not been completed, the waker
    /// field will be updated to the new waker if the old waker would not wake the same task.
    pub fn check(self, waker: &Waker) -> Result<io::Result<usize>, Completion> {
        unsafe {
            // Safe because our NonNull never dangles.
            let state: &State = self.state.as_ref();
            let mut waiter = Waiter::default();
            loop {
                match state.tag.compare_and_swap(SUBMITTED, UPDATING, Acquire) {
                    // In the updating state, wait for the lock to be released.
                    UPDATING    => waiter.wait(),

                    // In submitted state, possibly replace the waker then return self.
                    SUBMITTED   => {
                        // Cloning the waker calls an unknown function, which could possibly panic.
                        // Therefore, we use an Unlocker struct to unlock the state during
                        // unwinding even if this function panics.
                        let unlocker = Unlocker { tag: &state.tag };

                        // Safe because we have exclusive access in the UPDATING state, and we know
                        // we were in the SUBMITTED state, which held a waker.
                        let old_waker: &mut ManuallyDrop<Waker> = &mut (*state.data.get()).waker;

                        // We only need to replace the waker if the old waker will not wake the
                        // same task as the new waker.
                        if !old_waker.will_wake(waker) {
                            // Clone the new waker and replace the old one. Once that is done, we
                            // no longer need to hold the lock, so we drop the unlocker before
                            // dropping the old waker.
                            let waker = waker.clone();
                            let old_waker = mem::replace(old_waker, ManuallyDrop::new(waker));
                            drop(unlocker);
                            ManuallyDrop::into_inner(old_waker);
                        } else {
                            drop(unlocker);
                        }

                        return Err(self);
                    }

                    // In the completed state, return the result and drop the completion.
                    COMPLETED   => {
                        let result = (*state.data.get()).result;
                        drop(state);
                        drop(Box::from_raw(self.state.as_ptr()));
                        match result >= 0 {
                            true    => return Ok(Ok(result as usize)),
                            false   => return Ok(Err(io::Error::from_raw_os_error(result))),
                        }
                    }

                    // no other state should be possible
                    _           => debug_assert!(false),
                }
            }
        }
    }

    /// Cancel interest in this completion. The Cancellation callback will be stored to clean up
    /// resources shared with the kernel when the event completes.
    pub fn cancel(self, callback: Cancellation) {
        unsafe {
            // Safe because our NonNull never dangles.
            let state: &State = self.state.as_ref();
            let mut waiter = Waiter::default();
            loop {
                match state.tag.compare_and_swap(SUBMITTED, UPDATING, Acquire) {
                    // In the updating state, wait for the lock to be released.
                    UPDATING    => waiter.wait(),

                    // In the submitted state, replace the waker with the cancellation callback.
                    SUBMITTED   => {
                        let callback = ManuallyDrop::new(callback);

                        // Safe because we know we have exclusive access to the completion and it
                        // was in the submitted state.
                        let waker = mem::replace(&mut (*state.data.get()), Data { callback }).waker;

                        // We release the lock before dropping the waker, to reduce the length of
                        // the critical section and to avoid concern about panics in the Waker's
                        // drop function.
                        state.tag.store(CANCELLED, Release);

                        drop(ManuallyDrop::into_inner(waker));
                        return;
                    }

                    // In the completed state, clean up all resources.
                    COMPLETED => {
                        // We drop our own state before the callback to make sure we are not leaked
                        // if the callback's destructor panics.
                        drop(state);
                        drop(Box::from_raw(self.state.as_ptr()));
                        drop(callback);
                        return;
                    }
                    _           => debug_assert!(false),
                }
            }
        }
    }
}

unsafe impl Send for Completion { }
unsafe impl Sync for Completion { }

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            match self.tag.load(Acquire) {
                // Safe because we know we are in the cancelled state and we are being dropped.
                CANCELLED   => ManuallyDrop::drop(&mut (*self.data.get()).callback),

                // Safe because we know we are in the submitted state and we are being dropped.
                SUBMITTED   => ManuallyDrop::drop(&mut (*self.data.get()).waker),

                _           => { }
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

    let result = cqe.raw_result();
    let completion = cqe.user_data() as *mut State;

    if completion != ptr::null_mut() {
        // Safe because our state never dangles.
        let state: &State = &*completion;
        let mut waiter = Waiter::default();
        loop {
            match state.tag.compare_and_swap(SUBMITTED, UPDATING, Acquire) {
                // In the updating state, wait for the lock to be released.
                UPDATING    => waiter.wait(),

                // In the submitted state, replace the waker with the result and wake
                SUBMITTED   => {
                    // Safe because we know we were in the submitted state and we have exlcusive
                    // access
                    let waker = mem::replace(&mut (*state.data.get()), Data { result }).waker;

                    // We unlock before we wake so that we don't have to worry about panics and we
                    // reduce the length of the critical section
                    state.tag.store(COMPLETED, Release);
                    ManuallyDrop::into_inner(waker).wake();
                    return;
                }

                // In the cancelled state, clean up all resources
                CANCELLED   => {
                    drop(state);
                    drop(Box::from_raw(completion));
                    return;
                }
                _           => debug_assert!(false),
            }
        }
    }
}

// A waiter waits before attempting to acquire a lock again, first by busy waiting and then by
// yielding to the OS scheduler
#[derive(Default)]
struct Waiter {
    iteration: u8,
}

impl Waiter {
    fn wait(&mut self) {
        if self.iteration <= 3 {
            for _ in 0..(1 << self.iteration) {
                atomic::spin_loop_hint();
            }
            self.iteration += 1;
        } else {
            thread::yield_now();
        }
    }
}

// An unlocker unlocks the completion back to the submitted state in its destructor, to make sure
// we handle panics in Waker::clone well.
struct Unlocker<'a> {
    tag: &'a AtomicUsize,
}

impl<'a> Drop for Unlocker<'a> {
    fn drop(&mut self) {
        self.tag.store(SUBMITTED, Release);
    }
}
