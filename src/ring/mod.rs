mod engine;

use std::io;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;
use futures_io::{AsyncRead, AsyncBufRead, AsyncWrite};

use crate::driver::{Drive, Driver};

use engine::Engine;

pub struct Ring<IO, D = &'static Driver> {
    engine: Engine,
    io: IO,
    driver: D,
}

impl<IO: AsRawFd, D: Drive + Default> Ring<IO, D> {
    pub fn new(io: IO) -> Ring<IO, D> {
        Ring::on_driver(io, D::default())
    }
}

impl<IO: AsRawFd, D: Drive> Ring<IO, D> {
    pub fn on_driver(io: IO, driver: D) -> Ring<IO, D> {
        let engine = Engine::new();
        Ring { engine, io, driver }
    }

    /// Cancel interest in any ongoing IO.
    pub fn cancel(&mut self) {
        self.engine.cancel();
    }
}

impl<IO, D> Ring<IO, D> {
    fn split(self: Pin<&mut Self>) -> (&mut Engine, Pin<&mut IO>, Pin<&mut D>) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            let engine = &mut this.engine;
            let io = Pin::new_unchecked(&mut this.io);
            let driver = Pin::new_unchecked(&mut this.driver);
            (engine, io, driver)
        }
    }
}

impl<IO: io::Read + AsRawFd, D: Drive> AsyncBufRead for Ring<IO, D> {
    fn poll_fill_buf(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let (engine, io, driver) = self.split();
        engine.poll_fill_read_buf(ctx, driver, io.as_raw_fd())
    }
    
    fn consume(self: Pin<&mut Self>, amt: usize) {
        let (engine, ..) = self.split();
        engine.consume(amt);
    }
}

impl<IO: io::Read + AsRawFd, D: Drive> AsyncRead for Ring<IO, D> {
    fn poll_read(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<io::Result<usize>>
    {
        let mut inner = ready!(self.as_mut().poll_fill_buf(ctx))?;
        let len = io::Read::read(&mut inner, buf)?;
        self.consume(len);
        Poll::Ready(Ok(len))
    }
}

impl<IO: io::Write + AsRawFd, D: Drive> AsyncWrite for Ring<IO, D> {
    fn poll_write(self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let (engine, io, driver) = self.split();
        engine.poll_write(ctx, driver, io.as_raw_fd(), buf)
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let (engine, io, driver) = self.split();
        engine.poll_flush_write_buf(ctx, driver, io.as_raw_fd())
    }

    fn poll_close(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // TODO close underlying fd
        let (engine, io, driver) = self.split();
        engine.poll_flush_write_buf(ctx, driver, io.as_raw_fd())
    }
}
