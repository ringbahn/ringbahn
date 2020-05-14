mod engine;

use std::cmp;
use std::io;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;
use futures_io::{AsyncRead, AsyncBufRead};

use crate::event::Cancellation;
use crate::driver::{Drive, Driver};

use engine::Engine;

pub struct Ring<IO, D = &'static Driver> {
    engine: Engine<D>,
    io: IO,
    buf: Buffer,
}

impl<IO: AsRawFd + io::Read, D: Drive + Default> Ring<IO, D> {
    pub fn new(io: IO) -> Ring<IO, D> {
        Ring::on_driver(io, D::default())
    }
}

impl<IO: AsRawFd + io::Read, D: Drive> Ring<IO, D> {
    pub fn on_driver(io: IO, driver: D) -> Ring<IO, D> {
        Ring {
            engine: Engine::new(driver),
            buf: Buffer::new(&io),
            io, 
        }
    }

    fn split(self: Pin<&mut Self>) -> (&mut Buffer, Pin<&mut IO>, Pin<&mut Engine<D>>) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (&mut this.buf, Pin::new_unchecked(&mut this.io), Pin::new_unchecked(&mut this.engine))
        }
    }

    fn buffer(self: Pin<&mut Self>) -> &mut Buffer {
        unsafe { &mut Pin::get_unchecked_mut(self).buf }
    }
}

impl<IO: io::Read + AsRawFd, D: Drive> AsyncBufRead for Ring<IO, D> {
    fn poll_fill_buf(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let (buf, io, engine) = self.split();
        if buf.pos >= buf.cap {
            buf.cap = unsafe {
                ready!(engine.poll_read(ctx, io.as_raw_fd(), &mut buf.buf[..]))?
            };
            buf.pos = 0;
        }
        Poll::Ready(Ok(&buf.buf[buf.pos..buf.cap]))
    }
    
    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.buffer().consume(amt);
    }
}

impl<IO: io::Read + AsRawFd, D: Drive> AsyncRead for Ring<IO, D> {
    fn poll_read(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<io::Result<usize>>
    {
        match self.as_mut().poll_fill_buf(ctx) {
            Poll::Ready(Ok(inner))  => {
                let len = cmp::min(inner.len(), buf.len());
                buf[0..len].copy_from_slice(&inner[0..len]);
                self.as_mut().consume(len);
                Poll::Ready(Ok(len))
            }
            Poll::Ready(Err(err))   => Poll::Ready(Err(err)),
            Poll::Pending           => Poll::Pending,
        }
    }
}

struct Buffer {
    buf: Box<[u8]>,
    pos: usize,
    cap: usize,
}

impl Buffer {
    fn new<IO: io::Read>(#[allow(unused_variables)] io: &IO) -> Buffer {
        unsafe {
            const CAPACITY: usize = 1024 * 8;
            let mut buf = Vec::with_capacity(CAPACITY);
            buf.set_len(CAPACITY);

            #[cfg(feature = "nightly")] {
                let initializer = io.initializer();
                if initializer.should_initialize() {
                    initializer.initialize(&mut buf[..]);
                }
            }

            #[cfg(not(feature = "nightly"))] {
                std::ptr::write_bytes(buf.as_mut_ptr(), 0, CAPACITY)
            }

            Buffer {
                buf: buf.into_boxed_slice(),
                pos: 0,
                cap: 0,
            }
        }
    }

    fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.cap);
    }
}

impl<IO, D> Drop for Ring<IO, D> {
    fn drop(&mut self) {
        unsafe {
            let data = self.buf.buf.as_mut_ptr();
            let len = self.buf.buf.len();
            self.engine.cancel(Cancellation::buffer(data, len));
        }
    }
}
