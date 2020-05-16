use std::future::Future;
use std::mem;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::task::{Context, Poll};

use super::waker_set::WakerSet;

pub struct AccessQueue<T> {
    count: AtomicUsize,
    wakers: WakerSet,
    guarded: T,
}

const SENTINEL_KEY: usize = usize::MAX;

impl<T> AccessQueue<T> {
    pub fn new(guarded: T, accesses: usize) -> AccessQueue<T> {
        AccessQueue {
            count: AtomicUsize::new(accesses),
            wakers: WakerSet::new(),
            guarded,
        }
    }

    pub fn enqueue(&self) -> WillAccess<'_, T> {
        WillAccess {
            queue: self,
            key: SENTINEL_KEY,
        }
    }

    /// Access the guarded item without waiting in the queue.
    ///
    /// This bypasses the access queue entirely; it does not count as an access
    /// in the queue at all. 
    #[inline(always)]
    pub fn skip_queue(&self) -> &T {
        &self.guarded
    }

    /// Attempt to block some number of accesses.
    ///
    /// This method can fail to block those accesses if there were not that many accesses
    /// available to block.
    ///
    /// Returns true if it successfully blocked that many accesses. Returns false otherwise;
    /// if it returns false, the number of accesses in the queue remains unchanged.
    pub fn block(&self, amt: usize) -> bool {
        let mut current = self.count.load(SeqCst);
        while current >= amt {
            match self.count.compare_exchange_weak(current, current - amt, SeqCst, SeqCst) {
                Ok(_)   => return true,
                Err(n)  => current = n,
            }
        }
        false
    }

    /// Release some number of accesses to the queue.
    pub fn release(&self, amt: usize) {
        self.count.fetch_add(amt, SeqCst);
        self.wakers.notify(amt);
    }

    fn is_free(&self) -> bool {
        self.count.load(SeqCst) > 0
    }
}

pub struct WillAccess<'a, T> {
    queue: &'a AccessQueue<T>,
    key: usize,
}

impl<'a, T> WillAccess<'a, T> {
    pub fn skip_queue(&self) -> &T {
        self.queue.skip_queue()
    }
}

impl<'a, T> Future for WillAccess<'a, T> {
    type Output = AccessGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<AccessGuard<'a, T>> {
        // Remove any previously registered waker from the queue
        if self.key != SENTINEL_KEY {
            self.queue.wakers.remove(self.key);
        }

        // Attempt to take 1 access to the queue; insert a waker in the queue if
        // its not possible
        while !self.queue.block(1) {
            self.key = self.queue.wakers.insert(ctx);

            // If the queue still isn't free, return pending
            if !self.queue.is_free() {
                return Poll::Pending;
            }
        }

        Poll::Ready(AccessGuard { queue: self.queue })
    }
}

impl<'a, T> Drop for WillAccess<'a, T> {
    fn drop(&mut self) {
        // Cancel interest in access if this future is waiting on access
        if self.key != SENTINEL_KEY {
            self.queue.wakers.cancel(self.key);
        }
    }
}

pub struct AccessGuard<'a, T> {
    queue: &'a AccessQueue<T>
}

impl<'a, T> AccessGuard<'a, T> {
    pub fn hold_indefinitely(self) {
        mem::forget(self)
    }
}

impl<'a, T> Deref for AccessGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.queue.skip_queue()
    }
}

impl<'a, T> Drop for AccessGuard<'a, T> {
    fn drop(&mut self) {
        self.queue.release(1);
    }
}
