//! Interact with the file system using io-uring

use std::convert::TryFrom;
use std::fs;
use std::future::Future;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use either::Either;
use futures_core::ready;
use futures_io::{AsyncBufRead, AsyncRead, AsyncSeek, AsyncWrite};
use iou::sqe::{Mode, OFlag};

use crate::buf::Buffer;
use crate::drive::demo::DemoDriver;
use crate::drive::Drive;
use crate::event::OpenAt;
use crate::ring::{Cancellation, RawFdCancellation, Ring};
use crate::Submission;

type FileBuf = Either<Buffer, Box<libc::statx>>;

/// A file handle that runs on io-uring
pub struct File<D: Drive = DemoDriver> {
    ring: Ring<D>,
    fd: RawFd,
    active: Op,
    pending: bool,
    buf: FileBuf,
    pos: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Op {
    Nothing,
    Read,
    Write,
    Close,
    Statx,
    Closed,
}

impl File {
    /// Open a file using the default driver
    pub fn open(path: impl AsRef<Path>) -> Open {
        File::open_on_driver(path, DemoDriver::default())
    }

    /// Create a new file using the default driver
    pub fn create(path: impl AsRef<Path>) -> Create {
        File::create_on_driver(path, DemoDriver::default())
    }
}

impl<D: Drive + Clone> File<D> {
    /// Open a file
    pub fn open_on_driver(path: impl AsRef<Path>, driver: D) -> Open<D> {
        let flags = OFlag::O_CLOEXEC | OFlag::O_RDONLY;
        Open(driver.submit(OpenAt::without_dir(
            path,
            flags,
            Mode::from_bits(0o666).unwrap(),
        )))
    }

    /// Create a file
    pub fn create_on_driver(path: impl AsRef<Path>, driver: D) -> Create<D> {
        let flags = OFlag::O_CLOEXEC | OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
        Create(driver.submit(OpenAt::without_dir(
            path,
            flags,
            Mode::from_bits(0o666).unwrap(),
        )))
    }
}

impl<D: Drive> File<D> {
    /// Take an existing file and run its IO on an io-uring driver
    pub fn run_on_driver(file: fs::File, driver: D) -> File<D> {
        let file = ManuallyDrop::new(file);
        File::from_fd(file.as_raw_fd(), driver)
    }

    fn from_fd(fd: RawFd, driver: D) -> File<D> {
        File {
            ring: Ring::new(driver),
            active: Op::Nothing,
            pending: false,
            buf: Either::Left(Buffer::default()),
            pos: 0,
            fd,
        }
    }

    /// Access any data that has been read into the buffer, but not consumed
    ///
    /// This is similar to the fill_buf method from AsyncBufRead, but instead of performing IO if
    /// the buffer is empty, it will just return an empty slice. This method can be used to copy
    /// out any left over buffered data before closing or performing a write.
    pub fn read_buffered(&self) -> &[u8] {
        if self.active == Op::Read {
            self.buf.as_ref().unwrap_left().buffered_from_read()
        } else {
            &[]
        }
    }

    fn guard_op(self: Pin<&mut Self>, op: Op) {
        let (ring, buf, _pos, active, pending) = self.split();
        if *active == Op::Closed || *active == Op::Close {
            panic!("Attempted to perform IO on a closed File");
        } else if *pending {
            let new_buf = if op == Op::Statx {
                Either::Right(Box::new(unsafe { mem::zeroed() }))
            } else {
                Either::Left(Buffer::default())
            };
            ring.cancel_pinned(Cancellation::from(mem::replace(buf, new_buf)));
        }

        *active = op;
        *pending = true;
    }

    fn cancel(&mut self) {
        if self.pending {
            let new_buf = Either::Left(Buffer::default());
            self.ring
                .cancel(Cancellation::from(mem::replace(&mut self.buf, new_buf)));
            self.active = Op::Nothing;
            self.pending = false;
        }
    }

    fn poll_file_size(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        static EMPTY: libc::c_char = 0;
        use std::ffi::CStr;

        self.as_mut().guard_op(Op::Statx);
        let fd = self.fd();
        let (ring, statx, _pos, pending) = self.split_with_statx();
        let flags = iou::sqe::StatxFlags::AT_EMPTY_PATH;
        let mask = iou::sqe::StatxMode::STATX_SIZE;
        ready!(ring.poll(ctx, |sqe| unsafe {
            sqe.prep_statx(fd, CStr::from_ptr(&EMPTY), flags, mask, statx)
        }))?;
        *pending = false;
        Poll::Ready(Ok((*statx).stx_size))
    }

    #[inline(always)]
    fn split(
        self: Pin<&mut Self>,
    ) -> (
        Pin<&mut Ring<D>>,
        &mut FileBuf,
        &mut u64,
        &mut Op,
        &mut bool,
    ) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (
                Pin::new_unchecked(&mut this.ring),
                &mut this.buf,
                &mut this.pos,
                &mut this.active,
                &mut this.pending,
            )
        }
    }

    #[inline(always)]
    fn split_with_buf(
        self: Pin<&mut Self>,
    ) -> (Pin<&mut Ring<D>>, &mut Buffer, &mut u64, &mut bool) {
        let (ring, buf, pos, _active, pending) = self.split();
        if buf.is_right() {
            *buf = Either::Left(Buffer::default());
        }
        let buf = buf.as_mut().unwrap_left();
        (ring, buf, pos, pending)
    }

    #[inline(always)]
    fn split_with_statx(
        self: Pin<&mut Self>,
    ) -> (Pin<&mut Ring<D>>, &mut libc::statx, &mut u64, &mut bool) {
        let (ring, buf, pos, _active, pending) = self.split();
        if buf.is_left() {
            *buf = Either::Right(Box::new(unsafe { mem::zeroed() }));
        }
        let statx = buf.as_mut().unwrap_right();
        (ring, statx, pos, pending)
    }

    #[inline(always)]
    fn ring(self: Pin<&mut Self>) -> Pin<&mut Ring<D>> {
        self.split().0
    }

    #[inline(always)]
    fn buf(self: Pin<&mut Self>) -> &mut Buffer {
        self.split_with_buf().1
    }

    #[inline(always)]
    fn pos(self: Pin<&mut Self>) -> &mut u64 {
        self.split().2
    }

    #[inline(always)]
    fn fd(&self) -> RawFd {
        debug_assert!(self.fd >= 0);
        self.fd
    }

    fn confirm_close(self: Pin<&mut Self>, succeed: bool) {
        let this = unsafe { Pin::get_unchecked_mut(self) };

        this.pending = false;
        if succeed {
            this.active = Op::Closed;
            this.fd = -1;
        } else {
            this.active = Op::Nothing;
        }
    }
}

impl<D: Drive> AsyncRead for File<D> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut inner = ready!(self.as_mut().poll_fill_buf(ctx))?;
        let len = io::Read::read(&mut inner, buf)?;
        self.consume(len);
        Poll::Ready(Ok(len))
    }
}

impl<D: Drive> AsyncBufRead for File<D> {
    fn poll_fill_buf(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        self.as_mut().guard_op(Op::Read);
        let fd = self.fd();
        let (ring, buf, pos, pending) = self.split_with_buf();
        buf.fill_buf(|buf| {
            let n = ready!(ring.poll(ctx, |sqe| unsafe { sqe.prep_read(fd, buf, *pos) }))?;
            *pending = false;
            *pos += n as u64;
            Poll::Ready(Ok(n as u32))
        })
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.buf().consume(amt);
    }
}

impl<D: Drive> AsyncWrite for File<D> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        slice: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.as_mut().guard_op(Op::Write);
        let fd = self.fd();
        let (ring, buf, pos, pending) = self.split_with_buf();
        let data =
            ready!(buf.fill_buf(|mut buf| {
                Poll::Ready(Ok(io::Write::write(&mut buf, slice)? as u32))
            }))?;
        let n = ready!(ring.poll(ctx, |sqe| unsafe { sqe.prep_write(fd, data, *pos) }))?;
        *pending = false;
        *pos += n as u64;
        buf.clear();
        Poll::Ready(Ok(n as usize))
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_write(ctx, &[]))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.as_mut().guard_op(Op::Close);
        let fd = self.fd();
        match ready!(self
            .as_mut()
            .ring()
            .poll(ctx, |sqe| unsafe { sqe.prep_close(fd) }))
        {
            Ok(_) => {
                self.confirm_close(true);
                Poll::Ready(Ok(()))
            }
            Err(e) => {
                self.confirm_close(false);
                Poll::Ready(Err(e))
            }
        }
    }
}

impl<D: Drive> AsyncSeek for File<D> {
    fn poll_seek(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
        pos: io::SeekFrom,
    ) -> Poll<io::Result<u64>> {
        let (whence, offset) = match pos {
            io::SeekFrom::Start(n) => {
                *self.as_mut().pos() = n;
                return Poll::Ready(Ok(self.pos));
            }
            io::SeekFrom::Current(n) => (self.pos, n),
            io::SeekFrom::End(n) => (ready!(self.as_mut().poll_file_size(ctx))?, n),
        };
        let valid_seek = if offset.is_negative() {
            match whence.checked_sub(offset.abs() as u64) {
                Some(valid_seek) => valid_seek,
                None => {
                    let invalid = io::Error::from(io::ErrorKind::InvalidInput);
                    return Poll::Ready(Err(invalid));
                }
            }
        } else {
            match whence.checked_add(offset as u64) {
                Some(valid_seek) => valid_seek,
                None => {
                    let overflow = io::Error::from_raw_os_error(libc::EOVERFLOW);
                    return Poll::Ready(Err(overflow));
                }
            }
        };
        *self.as_mut().pos() = valid_seek;
        Poll::Ready(Ok(self.pos))
    }
}

impl From<fs::File> for File {
    fn from(file: fs::File) -> File {
        File::run_on_driver(file, DemoDriver::default())
    }
}

impl<D: Drive> TryFrom<File<D>> for fs::File {
    type Error = std::io::Error;

    fn try_from(mut file: File<D>) -> Result<Self, Self::Error> {
        // Reject when the file descriptor has been closed or there's an inflight close() request.
        if file.active == Op::Closed || file.active == Op::Close || file.fd < 0 {
            Err(io::Error::from_raw_os_error(libc::EBADF))
        } else {
            file.cancel();
            file.active = Op::Closed;
            unsafe { Ok(fs::File::from_raw_fd(file.fd)) }
        }
    }
}

impl<D: Drive> Drop for File<D> {
    fn drop(&mut self) {
        if self.pending {
            let new_buf = Either::Left(Buffer::default());
            let buf = mem::replace(&mut self.buf, new_buf);
            let buf = Box::new(Cancellation::from(buf));
            let fd = RawFdCancellation::new(self.fd(), self.active == Op::Close);
            self.ring.cancel(Cancellation::from((buf, fd)));
        } else if self.active != Op::Closed && self.fd >= 0 {
            unsafe { libc::close(self.fd) };
        }
    }
}

/// A future representing an opening file.
pub struct Open<D: Drive = DemoDriver>(Submission<OpenAt, D>);

impl<D: Drive> Open<D> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut Submission<OpenAt, D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.0) }
    }
}

impl<D: Drive + Clone> Future for Open<D> {
    type Output = io::Result<File<D>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<File<D>>> {
        let mut inner = self.inner();
        let (_, result) = ready!(inner.as_mut().poll(ctx));
        let fd = result? as i32;
        let driver = inner.driver().clone();
        Poll::Ready(Ok(File::from_fd(fd, driver)))
    }
}

/// A future representing a file being created.
pub struct Create<D: Drive = DemoDriver>(Submission<OpenAt, D>);

impl<D: Drive> Create<D> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut Submission<OpenAt, D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.0) }
    }
}

impl<D: Drive + Clone> Future for Create<D> {
    type Output = io::Result<File<D>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<File<D>>> {
        let mut inner = self.inner();
        let (_, result) = ready!(inner.as_mut().poll(ctx));
        let fd = result? as i32;
        let driver = inner.driver().clone();
        Poll::Ready(Ok(File::from_fd(fd, driver)))
    }
}
