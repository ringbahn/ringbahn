use std::borrow::Cow;
use std::future::Future;
use std::io;
use std::os::unix::io::RawFd;
use std::pin::Pin;
use std::task::{Poll, Context};

use futures_core::ready;

use crate::{Drive, Ring};

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
pub async fn __eprint<D: Drive>(driver: D, bytes: Cow<'static, [u8]>) {
    Print {
        ring: Ring::new(driver),
        fd: 2,
        bytes,
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
                let written = ready!(ring.as_mut().poll(ctx, true, 1, |sqs| unsafe {
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
