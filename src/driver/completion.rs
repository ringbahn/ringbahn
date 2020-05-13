use std::io;
use std::mem;
use std::ptr;
use std::task::Waker;

use parking_lot::Mutex;

use crate::event::Cancellation;

pub struct Completion {
    state: Mutex<State>,
}

enum State {
    Submitted(Waker),
    Completed(i32),
    Cancelled(Cancellation),
}

impl Completion {
    pub(crate) fn new(waker: Waker) -> Completion {
        Completion {
            state: Mutex::new(State::Submitted(waker)),
        }
    }

    pub(crate) fn set_waker(&mut self, waker: Waker) {
        let mut state = self.state.lock();
        if let State::Submitted(slot) = &mut *state {
            *slot = waker;
        }
    }

    pub(crate) fn cancel(&self, mut callback: Cancellation) {
        unsafe {
            let mut state = self.state.lock();
            if matches!(&*state, State::Completed(_)) {
                drop(Box::from_raw(self as *const Completion as *mut Completion));
                callback.cancel();
            } else {
                *state = State::Cancelled(callback);
            }
        }
    }

    pub(crate) fn check(&self) -> Option<io::Result<usize>> {
        let state = self.state.lock();
        match *state {
            State::Completed(result)    => {
                match result >= 0 {
                    true    => Some(Ok(result as usize)),
                    false   => Some(Err(io::Error::from_raw_os_error(-result))),
                }
            }
            _                           => None,
        }
    }
}

pub unsafe fn complete(cqe: iou::CompletionQueueEvent) {
    if cqe.is_timeout() { return; }

    let completion = cqe.user_data() as *mut Completion;

    if completion != ptr::null_mut() {
        let mut state = (*completion).state.lock();
        match mem::replace(&mut *state, State::Completed(cqe.raw_result())) {
            State::Submitted(waker)         => waker.wake(),
            State::Cancelled(mut callback)  => {
                drop(Box::from_raw(completion));
                callback.cancel();
            }
            State::Completed(_)         => panic!()
        }
    }
}
