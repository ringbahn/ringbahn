use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::task::{Context, Poll};

use futures_core::ready;
use event_listener::{Event, EventListener};

pub struct AccessController {
    count: AtomicUsize,
    event: Event,
}

impl AccessController {
    pub fn new(count: usize) -> AccessController {
        AccessController {
            count: AtomicUsize::new(count),
            event: Event::new(),
        }
    }

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

    pub fn release(&self, amt: usize) {
        self.count.fetch_add(amt, SeqCst);
        self.event.notify(amt);
    }

    pub fn access(&self) -> Access<'_> {
        Access {
            listener: None,
            controller: self,
        }
    }
}

pub struct Access<'a> {
    controller: &'a AccessController,
    listener: Option<EventListener>,
}

impl<'a> Future for Access<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        if let Some(listener) = &mut self.listener {
            ready!(Pin::new(listener).poll(ctx));
            self.listener = None;
        }

        while !self.controller.block(1) {
            match &mut self.listener {
                Some(listener)  => ready!(Pin::new(listener).poll(ctx)),
                None            => {
                    let mut listener = self.controller.event.listen();
                    if let Poll::Pending = Pin::new(&mut listener).poll(ctx) {
                        self.listener = Some(listener);
                        return Poll::Pending
                    }
                }
            }
        }

        Poll::Ready(())
    }
}
