mod engine;

use std::io;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;
use futures_io::{AsyncRead, AsyncBufRead, AsyncWrite};

use crate::driver::{Drive, Driver};

use engine::Engine;

pub trait AsyncBufWrite {
    fn poll_partial_flush_buf(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&mut [u8]>>;

    fn produce(self: Pin<&mut Self>, amt: usize);
}

pub struct Ring<IO, D = &'static Driver> {
    engine: Engine,
    io: IO,
    driver: D,
}

impl<IO: AsRawFd + io::Read, D: Drive + Default> Ring<IO, D> {
    pub fn new(io: IO) -> Ring<IO, D> {
        Ring::on_driver(io, D::default())
    }
}

impl<IO: AsRawFd + io::Read, D: Drive> Ring<IO, D> {
    pub fn on_driver(io: IO, driver: D) -> Ring<IO, D> {
        Ring {
            engine: Engine::new(&io),
            io, driver,
        }
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

impl<IO: io::Write + AsRawFd, D: Drive> AsyncBufWrite for Ring<IO, D> {
    fn poll_partial_flush_buf(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&mut [u8]>> {
        let (engine, io, driver) = self.split();
        engine.poll_partial_flush_write_buf(ctx, driver, io.as_raw_fd())
    }

    fn produce(self: Pin<&mut Self>, amt: usize) {
        let (engine, ..) = self.split();
        engine.produce(amt);
    }
}

impl<IO: io::Write + AsRawFd, D: Drive> AsyncWrite for Ring<IO, D> {
    fn poll_write(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        // TODO consider only flushing if strictly necessary
        let mut inner = ready!(self.as_mut().poll_partial_flush_buf(ctx))?;
        let len = io::Write::write(&mut inner, buf)?;
        self.produce(len);
        Poll::Ready(Ok(len))
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
