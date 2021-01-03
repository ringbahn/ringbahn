use std::future::Future;
use std::io;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::{ready, Stream};
use nix::sys::socket::{self as nix_socket, SockFlag};

use crate::drive::{demo::DemoDriver, Drive};
use crate::ring::{Cancellation, Ring};

use super::UnixStream;

pub struct UnixListener<D: Drive = DemoDriver> {
    ring: Ring<D>,
    fd: RawFd,
    active: Op,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum Op {
    Nothing = 0,
    Accept,
    Close,
    Closed,
}

impl UnixListener {
    pub fn bind(path: impl AsRef<Path>) -> io::Result<UnixListener> {
        UnixListener::bind_on_driver(path, DemoDriver::default())
    }
}

impl<D: Drive> UnixListener<D> {
    pub fn bind_on_driver(path: impl AsRef<Path>, driver: D) -> io::Result<UnixListener<D>> {
        let fd = super::socket()?;
        let addr = iou::sqe::SockAddr::Unix(nix_socket::UnixAddr::new(path.as_ref()).unwrap());
        nix_socket::bind(fd, &addr).map_err(|e| e.as_errno().unwrap_or(nix::errno::Errno::EIO))?;
        nix_socket::listen(fd, 128).map_err(|e| e.as_errno().unwrap_or(nix::errno::Errno::EIO))?;
        let ring = Ring::new(driver);
        Ok(UnixListener {
            active: Op::Nothing,
            fd,
            ring,
        })
    }

    pub fn close(&mut self) -> Close<D>
    where
        D: Unpin,
    {
        Pin::new(self).close_pinned()
    }

    pub fn close_pinned(self: Pin<&mut Self>) -> Close<D> {
        Close { socket: self }
    }

    fn guard_op(self: Pin<&mut Self>, op: Op) {
        let this = unsafe { Pin::get_unchecked_mut(self) };
        if this.active == Op::Closed {
            panic!("Attempted to perform IO on a closed UnixListener");
        }
        if this.active != Op::Nothing && this.active != op {
            this.cancel();
        }
        this.active = op;
    }

    fn cancel(&mut self) {
        if !matches!(self.active, Op::Nothing | Op::Closed) {
            self.active = Op::Nothing;
            self.ring.cancel(Cancellation::from(()));
        }
    }

    fn ring(self: Pin<&mut Self>) -> Pin<&mut Ring<D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.ring) }
    }

    fn confirm_close(self: Pin<&mut Self>) {
        unsafe {
            Pin::get_unchecked_mut(self).active = Op::Closed;
        }
    }
}

impl<D: Drive + Clone> UnixListener<D> {
    pub fn accept(&mut self) -> Accept<'_, D>
    where
        D: Unpin,
    {
        Pin::new(self).accept_pinned()
    }

    pub fn accept_pinned(self: Pin<&mut Self>) -> Accept<'_, D> {
        Accept { socket: self }
    }

    pub fn incoming(&mut self) -> Incoming<'_, D>
    where
        D: Unpin,
    {
        Pin::new(self).incoming_pinned()
    }

    pub fn incoming_pinned(self: Pin<&mut Self>) -> Incoming<'_, D> {
        Incoming {
            accept: self.accept_pinned(),
        }
    }

    pub fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<io::Result<UnixStream<D>>> {
        self.as_mut().guard_op(Op::Accept);
        let fd = self.fd;
        let fd = ready!(self.as_mut().ring().poll(ctx, |sqe| unsafe {
            sqe.prep_accept(fd, None, SockFlag::empty())
        }))? as RawFd;
        Poll::Ready(Ok(UnixStream::from_fd(fd, self.ring().clone())))
    }
}

impl<D: Drive> Drop for UnixListener<D> {
    fn drop(&mut self) {
        match self.active {
            Op::Closed => {}
            Op::Nothing => unsafe {
                libc::close(self.fd);
            },
            _ => self.cancel(),
        }
    }
}

pub struct Accept<'a, D: Drive> {
    socket: Pin<&'a mut UnixListener<D>>,
}

impl<'a, D: Drive + Clone> Future for Accept<'a, D> {
    type Output = io::Result<UnixStream<D>>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        self.socket.as_mut().poll_accept(ctx)
    }
}

pub struct Incoming<'a, D: Drive> {
    accept: Accept<'a, D>,
}

impl<'a, D: Drive> Incoming<'a, D> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut Accept<'a, D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.accept) }
    }
}

impl<'a, D: Drive + Clone> Stream for Incoming<'a, D> {
    type Item = io::Result<UnixStream<D>>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = ready!(self.inner().poll(ctx));
        Poll::Ready(Some(next))
    }
}

pub struct Close<'a, D: Drive> {
    socket: Pin<&'a mut UnixListener<D>>,
}

impl<'a, D: Drive> Future for Close<'a, D> {
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.as_mut().guard_op(Op::Close);
        let fd = self.socket.fd;
        ready!(self
            .socket
            .as_mut()
            .ring()
            .poll(ctx, |sqe| unsafe { sqe.prep_close(fd) }))?;
        self.socket.as_mut().confirm_close();
        Poll::Ready(Ok(()))
    }
}
