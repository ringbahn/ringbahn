use std::borrow::Cow;
use std::future::Future;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::pin::Pin;
use std::task::{Poll, Context};

use futures_core::ready;
use futures_io::AsyncWrite;

use crate::buf::Buffer;
use crate::{Drive, ring::Ring};
use crate::drive::demo::DemoDriver;

#[macro_export]
macro_rules! print {
    ($driver:expr, $($arg:tt)*) => {{
        let mut s = format!($($arg)*);
        $crate::io::__print($driver, s.into_bytes())
    }};
}

#[macro_export]
macro_rules! println {
    ($driver:expr) => {$crate::io::__print($driver, b"\n")};
    ($driver:expr, $($arg:tt)*) => {{
        let mut s = format!($($arg)*);
        s.push('\n');
        $crate::io::__print($driver, s.into_bytes())
    }};
}

#[macro_export]
macro_rules! eprint {
    ($driver:expr, $($arg:tt)*) => {{
        let mut s = format!($($arg)*);
        $crate::io::__eprint($driver, s.into_bytes())
    }};
}

#[macro_export]
macro_rules! eprintln {
    ($driver:expr) => {$crate::io::__eprint($driver, b"\n")};
    ($driver:expr, $($arg:tt)*) => {{
        let mut s = format!($($arg)*);
        s.push('\n');
        $crate::io::__eprint($driver, s.into_bytes())
    }};
}

#[doc(hidden)]
pub async fn __print<D: Drive>(driver: D, bytes: impl Into<Cow<'static, [u8]>>) {
    Print {
        ring: Ring::new(driver),
        fd: 1,
        bytes: bytes.into(),
        idx: 0,
    }.await.expect("printing to stdout failed")
}

#[doc(hidden)]
pub async fn __eprint<D: Drive>(driver: D, bytes: impl Into<Cow<'static, [u8]>>) {
    Print {
        ring: Ring::new(driver),
        fd: 2,
        bytes: bytes.into(),
        idx: 0,
    }.await.expect("printing to stderr failed")
}

struct Print<D: Drive> {
    ring: Ring<D>,
    fd: RawFd,
    bytes: Cow<'static, [u8]>,
    idx: usize,
}

impl<D: Drive> Print<D> {
    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, RawFd, &[u8], &mut usize) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            let bytes = &this.bytes.as_ref()[this.idx..];
            (Pin::new_unchecked(&mut this.ring), this.fd, bytes, &mut this.idx)
        }
    }
}

impl<D: Drive> Future for Print<D> {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let (mut ring, fd, mut bytes, idx) = self.split();
        if !bytes.is_empty() {
            loop {
                let written = ready!(ring.as_mut().poll(ctx, 1, |sqs| unsafe {
                    let mut sqe = sqs.single().unwrap();
                    sqe.prep_write(fd, bytes, 0);
                    sqe
                }))? as usize;
                *idx += written;
                if written == bytes.len() {
                    return Poll::Ready(Ok(()));
                } else {
                    bytes = &bytes[written..];
                }
            }
        } else {
            return Poll::Ready(Ok(()));
        }
    }
}

/// A handle to the standard output of the current process.
pub struct Stdout<D: Drive> {
    ring: Ring<D>,
    buf: Buffer,
}

/// Constructs a new handle to the standard output of the current process using the demo driver.
/// ```no_run
/// use ringbahn::io;
///
/// # use futures::AsyncWriteExt;
/// # fn main() -> std::io::Result<()> { futures::executor::block_on(async {
/// io::stdout().write(b"hello, world").await?;
/// # Ok(())
/// # })
/// # }
/// ```
// TODO synchronization note?
pub fn stdout() -> Stdout<DemoDriver> {
    Stdout::run_on_driver(DemoDriver::default())
}

impl<D: Drive> Stdout<D> {
    pub fn run_on_driver(driver: D) -> Stdout<D> {
        Stdout {
            ring: Ring::new(driver),
            buf: Buffer::default(),
        }
    }

    #[inline(always)]
    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Buffer) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.ring), &mut this.buf)
        }
    }
}

impl<D: Drive> AsyncWrite for Stdout<D> {
    fn poll_write(self: Pin<&mut Self>, ctx: &mut Context<'_>, slice: &[u8])
        -> Poll<io::Result<usize>>
    {
        let fd = self.as_raw_fd();
        let (ring, buf, ..) = self.split();
        let data = ready!(buf.fill_buf(|mut buf| {
            Poll::Ready(Ok(io::Write::write(&mut buf, slice)? as u32))
        }))?;
        let n = ready!(ring.poll(ctx, 1, |sqs| {
            let mut sqe = sqs.single().unwrap();
            unsafe {
                sqe.prep_write(fd, data, 0);
            }
            sqe
        }))?;
        buf.clear();
        Poll::Ready(Ok(n as usize))
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_write(ctx, &[]))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(ctx)
    }
}

impl<D: Drive> AsRawFd for Stdout<D> {
    fn as_raw_fd(&self) -> RawFd {
        libc::STDOUT_FILENO
    }
}
