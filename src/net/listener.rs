use std::alloc::{alloc, dealloc, Layout};
use std::io;
use std::future::Future;
use std::mem;
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::io::{RawFd};
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll};

use futures_core::{ready, Stream};

use crate::drive::demo::DemoDriver;
use crate::Cancellation;
use crate::{Drive, Ring};

use super::{TcpStream, addr_from_c, addr_to_c, socket};

pub struct TcpListener<D: Drive = DemoDriver<'static>> {
    ring: Ring<D>,
    fd: RawFd,
    active: Op,
    addr: *mut libc::sockaddr_storage,
    len: *mut libc::socklen_t,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum Op {
    Nothing = 0,
    Accept,
    Close,
}

impl TcpListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener> {
        TcpListener::bind_on_driver(addr, DemoDriver::default())
    }
}

impl<D: Drive> TcpListener<D> {
    pub fn bind_on_driver<A: ToSocketAddrs>(addr: A, driver: D) -> io::Result<TcpListener<D>> {
        let (fd, addr) = socket(addr)?;
        let val = &1 as *const libc::c_int as *const libc::c_void;
        let len = mem::size_of::<libc::c_int>() as u32;
        unsafe {
            if libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, val, len) < 0 {
                return Err(io::Error::last_os_error());
            }

            let (mut addr, addrlen) = addr_to_c(addr);
            let addr = &mut *addr as *mut libc::sockaddr_storage as *mut libc::sockaddr;
            if libc::bind(fd, addr, addrlen) < 0 {
                return Err(io::Error::last_os_error());
            }

            if libc::listen(fd, 128) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let ring = Ring::new(driver);
        Ok(TcpListener {
            active: Op::Nothing,
            addr: ptr::null_mut(),
            len: ptr::null_mut(),
            fd, ring,
        })
    }

    pub fn close(&mut self) -> Close<D> where D: Unpin {
        Pin::new(self).close_pinned()
    }

    pub fn close_pinned(self: Pin<&mut Self>) -> Close<D> {
        Close { socket: self }
    }

    fn guard_op(self: Pin<&mut Self>, op: Op) {
        let this = unsafe { Pin::get_unchecked_mut(self) };
        if this.active != Op::Nothing && this.active != op {
            this.cancel();
        }
        this.active = op;
    }

    fn cancel(&mut self) {
        let cancellation = match self.active {
            Op::Accept  => {
                unsafe fn callback(addr: *mut (), addrlen: usize) {
                    dealloc(addr as *mut u8, Layout::new::<libc::sockaddr_storage>());
                    dealloc(addrlen as *mut u8, Layout::new::<libc::socklen_t>());
                }
                unsafe {
                    Cancellation::new(self.addr as *mut (), self.len as usize, callback)
                }
            }
            Op::Close   => Cancellation::null(),
            Op::Nothing => return,
        };
        self.active = Op::Nothing;
        self.ring.cancel(cancellation);
    }

    fn addr(self: Pin<&mut Self>) -> (*mut libc::sockaddr, *mut libc::socklen_t) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            if this.addr == ptr::null_mut() {
                this.addr = alloc(Layout::new::<libc::sockaddr_storage>()) as *mut _;
                this.len = alloc(Layout::new::<libc::socklen_t>()) as *mut _;
                *this.len = mem::size_of::<libc::sockaddr_storage>() as _;
            }

            (this.addr as *mut libc::sockaddr, this.len)
        }
    }

    unsafe fn drop_addr(self: Pin<&mut Self>) {
        let this = Pin::get_unchecked_mut(self);
        dealloc(mem::replace(&mut this.addr, ptr::null_mut()) as *mut u8, Layout::new::<libc::sockaddr_storage>());
        dealloc(mem::replace(&mut this.len, ptr::null_mut()) as *mut u8, Layout::new::<libc::socklen_t>());
    }

    fn ring(self: Pin<&mut Self>) -> Pin<&mut Ring<D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.ring) }
    }
}

impl<D: Drive + Clone> TcpListener<D> {
    pub fn accept(&mut self) -> Accept<'_, D> where D: Unpin {
        Pin::new(self).accept_pinned()
    }

    pub fn accept_pinned(self: Pin<&mut Self>) -> Accept<'_, D> {
        Accept { socket: self }
    }

    pub fn incoming(&mut self) -> Incoming<'_, D> where D: Unpin {
        Pin::new(self).incoming_pinned()
    }

    pub fn incoming_pinned(self: Pin<&mut Self>) -> Incoming<'_, D> {
        Incoming { accept: self.accept_pinned() }
    }

    pub fn poll_accept(mut self: Pin<&mut Self>, ctx: &mut Context<'_>)
        -> Poll<io::Result<(TcpStream<D>, SocketAddr)>>
    {
        self.as_mut().guard_op(Op::Accept);
        let fd = self.fd;
        let flags = 0;
        let (addr, addrlen) = self.as_mut().addr();
        let fd = ready!(self.as_mut().ring().poll(ctx, true, |sqe| unsafe {
            uring_sys::io_uring_prep_accept(sqe.raw_mut(), fd, addr, addrlen, flags);
        }))? as RawFd;
        let addr = unsafe {
            let result = addr_from_c(&*addr, *addrlen as usize);
            self.as_mut().drop_addr();
            result?
        };
        Poll::Ready(Ok((TcpStream::from_fd(fd, self.ring().clone()), addr)))
    }

}

impl<D: Drive> Drop for TcpListener<D> {
    fn drop(&mut self) {
        match self.active {
            Op::Nothing => unsafe { libc::close(self.fd); }
            _           => self.cancel(),
        }
    }
}

pub struct Accept<'a, D: Drive> {
    socket: Pin<&'a mut TcpListener<D>>,
}

impl<'a, D: Drive> Accept<'a, D> {
}

impl<'a, D: Drive + Clone> Future for Accept<'a, D> {
    type Output = io::Result<(TcpStream<D>, SocketAddr)>;

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
    type Item = io::Result<(TcpStream<D>, SocketAddr)>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = ready!(self.inner().poll(ctx));
        Poll::Ready(Some(next))
    }
}


pub struct Close<'a, D: Drive> {
    socket: Pin<&'a mut TcpListener<D>>,
}

impl<'a, D: Drive> Future for Close<'a, D> {
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.as_mut().guard_op(Op::Close);
        let fd = self.socket.fd;
        ready!(self.socket.as_mut().ring().poll(ctx, true, |sqe| unsafe {
            uring_sys::io_uring_prep_close(sqe.raw_mut(), fd);
        }))?;
        Poll::Ready(Ok(()))
    }
}
