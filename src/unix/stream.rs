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

use crate::drive::{Drive, demo::DemoDriver};
use crate::event;
use crate::ring::Ring;
use crate::Submission;

use super::{socket, socketpair};

use crate::net::TcpStream;

pub struct UnixStream<D: Drive = DemoDriver> {
    inner: TcpStream<D>,
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
            inner: TcpStream::from_fd(fd, ring),
        }
    }

    #[inline(always)]
    fn inner(self: Pin<&mut Self>) -> Pin<&mut TcpStream<D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.inner) }
    }
}

pub struct Connect<D: Drive = DemoDriver>(
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
    fn poll_read(self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<io::Result<usize>>
    {
        self.inner().poll_read(ctx, buf)
    }
}

impl<D: Drive> AsyncBufRead for UnixStream<D> {
    fn poll_fill_buf(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        self.inner().poll_fill_buf(ctx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.inner().consume(amt)
    }
}

impl<D: Drive> AsyncWrite for UnixStream<D> {
    fn poll_write(self: Pin<&mut Self>, ctx: &mut Context<'_>, slice: &[u8]) -> Poll<io::Result<usize>> {
        self.inner().poll_write(ctx, slice)
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner().poll_flush(ctx)
    }

    fn poll_close(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner().poll_close(ctx)
    }
}
