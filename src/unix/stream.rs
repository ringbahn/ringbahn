use std::io;
use std::future::Future;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;
use futures_io::{AsyncRead, AsyncBufRead, AsyncWrite};
use iou::sqe::SockAddr;
use nix::sys::socket::UnixAddr;

use crate::buf::Buffer;
use crate::drive::demo::DemoDriver;
use crate::{Drive, Ring};
use crate::event;
use crate::Submission;

use super::{socket, socketpair};

pub struct UnixStream<D: Drive = DemoDriver<'static>> {
    ring: Ring<D>,
    buf: Buffer,
    active: Op,
    fd: RawFd,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Op {
    Read,
    Write,
    Close,
    Nothing,
}

impl UnixStream {
    pub fn connect(path: &impl AsRef<Path>) -> Connect {
        UnixStream::connect_on_driver(path, DemoDriver::default())
    }

    pub fn pair() -> io::Result<(UnixStream, UnixStream)> {
        UnixStream::pair_on_driver(DemoDriver::default())
    }
}

impl<D: Drive + Clone> UnixStream<D> {
    pub fn connect_on_driver(path: &impl AsRef<Path>, driver: D) -> Connect<D> {
        let fd = match socket() {
            Ok(fd)  => fd,
            Err(e)  => return Connect(Err(Some(e))),
        };
        let addr = Box::new(SockAddr::Unix(UnixAddr::new(path.as_ref()).unwrap()));
        Connect(Ok(driver.submit(event::Connect { fd, addr })))
    }

    pub fn pair_on_driver(driver: D) -> io::Result<(UnixStream<D>, UnixStream<D>)> {
        let (fd1, fd2) = socketpair()?;
        let ring1 = Ring::new(driver.clone());
        let ring2 = Ring::new(driver);
        Ok((UnixStream::from_fd(fd1, ring1), UnixStream::from_fd(fd2, ring2)))
    }
}

impl<D: Drive> UnixStream<D> {
    pub(super) fn from_fd(fd: RawFd, ring: Ring<D>) -> UnixStream<D> {
        UnixStream {
            buf: Buffer::new(),
            active: Op::Nothing,
            fd, ring,
        }
    }

    fn guard_op(self: Pin<&mut Self>, op: Op) {
        let this = unsafe { Pin::get_unchecked_mut(self) };
        if this.active != Op::Nothing && this.active != op {
            this.cancel();
        }
        this.active = op;
    }

    fn cancel(&mut self) {
        self.active = Op::Nothing;
        self.ring.cancel(self.buf.cancellation());
    }

    #[inline(always)]
    fn ring(self: Pin<&mut Self>) -> Pin<&mut Ring<D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.ring) }
    }

    #[inline(always)]
    fn buf(self: Pin<&mut Self>) -> Pin<&mut Buffer> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.buf) }
    }

    #[inline(always)]
    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Buffer) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.ring), &mut this.buf)
        }
    }
}

pub struct Connect<D: Drive = DemoDriver<'static>>(
    Result<Submission<event::Connect, D>, Option<io::Error>>
);

impl<D: Drive + Clone> Future for Connect<D> {
    type Output = io::Result<UnixStream<D>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            match &mut Pin::get_unchecked_mut(self).0 {
                Ok(submission)  => {
                    let mut submission = Pin::new_unchecked(submission);
                    let (connect, result) = ready!(submission.as_mut().poll(ctx));
                    result?;
                    let driver = submission.driver().clone();
                    Poll::Ready(Ok(UnixStream::from_fd(connect.fd, Ring::new(driver))))
                }
                Err(err)        => {
                    let err = err.take().expect("polled Connect future after completion");
                    Poll::Ready(Err(err))
                }
            }
        }
    }
}

impl<D: Drive> AsyncRead for UnixStream<D> {
    fn poll_read(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<io::Result<usize>>
    {
        let mut inner = ready!(self.as_mut().poll_fill_buf(ctx))?;
        let len = io::Read::read(&mut inner, buf)?;
        self.consume(len);
        Poll::Ready(Ok(len))
    }
}

impl<D: Drive> AsyncBufRead for UnixStream<D> {
    fn poll_fill_buf(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        self.as_mut().guard_op(Op::Read);
        let fd = self.fd;
        let (ring, buf) = self.split();
        buf.fill_buf(|buf| {
            let n = ready!(ring.poll(ctx, true, 1, |sqs| unsafe { 
                let mut sqe = sqs.single().unwrap();
                sqe.prep_read(fd, buf, 0);
                sqe
            }))?;
            Poll::Ready(Ok(n as u32))
        })
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.buf().consume(amt);
    }
}

impl<D: Drive> AsyncWrite for UnixStream<D> {
    fn poll_write(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, slice: &[u8]) -> Poll<io::Result<usize>> {
        self.as_mut().guard_op(Op::Write);
        let fd = self.fd;
        let (ring, buf) = self.split();
        let data = ready!(buf.fill_buf(|mut buf| {
            Poll::Ready(Ok(io::Write::write(&mut buf, slice)? as u32))
        }))?;
        let n = ready!(ring.poll(ctx, true, 1, |sqs| unsafe {
            let mut sqe = sqs.single().unwrap();
            sqe.prep_write(fd, data, 0);
            sqe
        }))?;
        buf.clear();
        Poll::Ready(Ok(n as usize))
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_write(ctx, &[]))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.as_mut().guard_op(Op::Close);
        let fd = self.fd;
        ready!(self.ring().poll(ctx, true, 1, |sqs| unsafe {
            let mut sqe = sqs.single().unwrap();
            sqe.prep_close(fd);
            sqe
        }))?;
        Poll::Ready(Ok(()))
    }
}
