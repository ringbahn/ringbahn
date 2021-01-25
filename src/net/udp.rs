//! UDP bindings for `ringbahn`.

use std::io;
use std::future::Future;
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, RawFd};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use nix::sys::socket::InetAddr;
use iou::sqe::SockAddr;

use crate::Submission;
use crate::buf::Buffer;
use crate::event;
use crate::drive::{Drive, demo::DemoDriver};
use crate::ring::Ring;

/// A UDP socket run on `io-uring`.
pub struct UdpSocket<D: Drive = DemoDriver> {
    ring: Ring<D>,
    buf: Buffer,
    active: Op,
    pub inner: std::net::UdpSocket,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum Op {
    Nothing = 0,
    Recv,
    Send,
}

impl UdpSocket {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
        UdpSocket::bind_on_driver(addr, DemoDriver::default())
    }
}

impl<D: Drive> UdpSocket<D> {
    /// Creates a UDP socket from the given address that runs on the driver.
    ///
    /// Binding with a port number of 0 will request that the OS assigns a port to this socket. The
    /// port allocated can be queried via the [`local_addr`] method.
    ///
    /// [`local_addr`]: #method.local_addr
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { futures::executor::block_on(async {
    /// #
    /// use ringbahn::net::UdpSocket;
    /// use ringbahn::drive::demo::DemoDriver;
    ///
    /// let socket = UdpSocket::bind_on_driver("127.0.0.1:0", DemoDriver::default())?;
    /// # Ok(()) }) }
    /// ```
    pub fn bind_on_driver<A: ToSocketAddrs>(addr: A, driver: D) -> io::Result<UdpSocket<D>> {
        Ok(UdpSocket {
            inner: std::net::UdpSocket::bind(addr)?,
            active: Op::Nothing,
            buf: Buffer::default(),
            ring: Ring::new(driver),
        })
    }

    fn guard_op(self: Pin<&mut Self>, op: Op) {
        let (ring, buf, active) = self.split();
        if *active != Op::Nothing && *active != op {
            ring.cancel_pinned(buf.cancellation());
        }
        *active = op;
    }

    fn cancel(&mut self) {
        todo!("UdpSocket cancel");
    }

     #[inline(always)]
    fn ring(self: Pin<&mut Self>) -> Pin<&mut Ring<D>> {
        self.split().0
    }

    #[inline(always)]
    fn buf(self: Pin<&mut Self>) -> &mut Buffer {
        self.split().1
    }

    #[inline(always)]
    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Buffer, &mut Op) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.ring), &mut this.buf, &mut this.active)
        }
    }

    /// Returns the socket address that this socket was created from.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Returns the socket address of the remote peer this socket was connected to.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.buf().consume(amt);
    }
}

impl <D: Drive> UdpSocket<D> {
    pub fn poll_recv(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<io::Result<usize>>
    {
        self.as_mut().guard_op(Op::Recv);
        let fd = self.as_raw_fd();
        let (ring, inner, ..) = self.split();
        let mut data = ready!(inner.fill_buf(|buf| {
            let n = ready!(ring.poll(ctx, 1, |sqs| {
                let mut sqe = sqs.single().unwrap();
                unsafe {
                    sqe.prep_read(fd, buf, 0);
                }
                sqe
            }))?;
            Poll::Ready(Ok(n as u32))
        }))?;
        let len = io::Read::read(&mut data, buf)?;
        inner.consume(len);
        Poll::Ready(Ok(len))
    }

    pub fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> Recv<'a, D> {
        Recv { socket: self, buf }
    }
}

/// Future for the [`UdpSocket::recv`](crate::net::udp::UdpSocket::recv) method.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Recv<'a, D: Drive> {
    pub(crate) socket: &'a mut UdpSocket<D>,
    pub(crate) buf: &'a mut [u8],
}

impl<D: Drive> Recv<'_, D> {
    fn project(self: Pin<&mut Self>) -> (Pin<&mut UdpSocket<D>>, &mut [u8]) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.socket), &mut this.buf)
        }
    }
}

impl<D: Drive> Future for Recv<'_, D> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let (socket, buf) = self.project();
        socket.poll_recv(ctx, buf)
    }
}

impl<D: Drive + Clone> UdpSocket<D> {
    /// Connects the UDP socket to a remote address.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { futures::executor::block_on(async {
    ///	use ringbahn::net::UdpSocket;
    ///
    /// let mut socket = UdpSocket::bind("127.0.0.1:0")?;
    /// socket.connect("127.0.0.1:8080").await?;
    /// # Ok(()) }) }
    /// ```
    pub fn connect<A: ToSocketAddrs>(&mut self, addr: A) -> Connect<D> {
        // TODO: try all addresses, not just the first
        match addr.to_socket_addrs() {
            Ok(mut addr) => match addr.next() {
                Some(addr) => {
                    let addr = Box::new(SockAddr::Inet(InetAddr::from_std(&addr)));
                    Connect(Ok(self.ring.driver().clone().submit(event::Connect { fd: self.as_raw_fd(), addr })))
                }
                None => Connect(Err(Some(io::Error::new(io::ErrorKind::InvalidInput, "could not resolve to any addresses"))))
            },
            Err(e) => Connect(Err(Some(e)))
        }
    }
}

/// Future for the [`UdpSocket::connect`](crate::net::udp::UdpSocket::connect) method.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Connect<D: Drive = DemoDriver>(
    Result<Submission<event::Connect, D>, Option<io::Error>>
);

impl<D: Drive + Clone> Future for Connect<D> {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            Ok(mut submission)  => {
                let (_, result) = ready!(submission.as_mut().poll(ctx));
                result?;
                Poll::Ready(Ok(()))
            }
            Err(err)        => {
                let err = err.take().expect("polled Connect future after completion");
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<D: Drive> Connect<D> {
    fn project(self: Pin<&mut Self>)
        -> Result<Pin<&mut Submission<event::Connect, D>>, &mut Option<io::Error>>
    {
        unsafe {
            match &mut Pin::get_unchecked_mut(self).0 {
                Ok(submission)  => Ok(Pin::new_unchecked(submission)),
                Err(err)        => Err(err)
            }
        }
    }
}

impl<D: Drive> Drop for UdpSocket<D> {
    fn drop(&mut self) {
        match self.active {
            Op::Recv | Op::Send => self.cancel(),
            Op::Nothing => (),
        }
    }
}

impl<D: Drive> AsRawFd for UdpSocket<D> {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}
