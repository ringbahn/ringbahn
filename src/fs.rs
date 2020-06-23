//! Interact with the file system using io-uring

use std::fs;
use std::future::Future;
use std::io;
use std::mem::ManuallyDrop;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;
use futures_io::{AsyncRead, AsyncBufRead, AsyncWrite, AsyncSeek};

use crate::buf::Buffer;
use crate::drive::Drive;
use crate::drive::demo::DemoDriver;
use crate::ring::Ring;
use crate::event::OpenAt;
use crate::Submission;

/// A file handle that runs on io-uring
pub struct File<D: Drive = DemoDriver<'static>> {
    ring: Ring<D>,
    fd: RawFd,
    active: Op,
    buf: Buffer,
    pos: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Op {
    Read,
    Write,
    Close,
    Nothing,
    Statx,
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
        let flags = libc::O_CLOEXEC | libc::O_RDONLY;
        let event = OpenAt::new(path, libc::AT_FDCWD, flags, 0o666);
        Open(Submission::new(event, driver))
    }

    /// Create a file
    pub fn create_on_driver(path: impl AsRef<Path>, driver: D) -> Create<D> {
        let flags = libc::O_CLOEXEC | libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
        let event = OpenAt::new(path, libc::AT_FDCWD, flags, 0o666);
        Create(Submission::new(event, driver))
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
            buf: Buffer::new(),
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
            self.buf.buffered_from_read()
        } else { &[] }
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

    fn poll_file_size(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        static EMPTY: libc::c_char = 0;

        self.as_mut().guard_op(Op::Statx);
        let fd = self.fd;
        let (ring, buf, _) = self.split();
        let statx = buf.as_statx();
        let flags = libc::AT_EMPTY_PATH;
        let mask = libc::STATX_SIZE;
        unsafe {
            ready!(ring.poll(ctx, true, |sqe| sqe.prep_statx(fd, &EMPTY, flags, mask, statx)))?;
            Poll::Ready(Ok((*statx).stx_size))
        }
    }

    #[inline(always)]
    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Buffer, &mut u64) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.ring), &mut this.buf, &mut this.pos)
        }
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
    fn pos(self: Pin<&mut Self>) -> Pin<&mut u64> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.pos) }
    }
}

impl<D: Drive> AsyncRead for File<D> {
    fn poll_read(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<io::Result<usize>>
    {
        let mut inner = ready!(self.as_mut().poll_fill_buf(ctx))?;
        let len = io::Read::read(&mut inner, buf)?;
        self.consume(len);
        Poll::Ready(Ok(len))
    }
}

impl<D: Drive> AsyncBufRead for File<D> {
    fn poll_fill_buf(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        self.as_mut().guard_op(Op::Read);
        let fd = self.fd;
        let (ring, buf, pos) = self.split();
        buf.fill_buf(|buf| {
            let n = ready!(ring.poll(ctx, true, |sqe| unsafe { sqe.prep_read(fd, buf, *pos) }))?;
            *pos += n as u64;
            Poll::Ready(Ok(n as u32))
        })
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.buf().consume(amt);
    }
}

impl<D: Drive> AsyncWrite for File<D> {
    fn poll_write(mut self: Pin<&mut Self>, ctx: &mut Context<'_>, slice: &[u8]) -> Poll<io::Result<usize>> {
        self.as_mut().guard_op(Op::Write);
        let fd = self.fd;
        let (ring, buf, pos) = self.split();
        let data = ready!(buf.fill_buf(|mut buf| {
            Poll::Ready(Ok(io::Write::write(&mut buf, slice)? as u32))
        }))?;
        let n = ready!(ring.poll(ctx, true, |sqe| unsafe { sqe.prep_write(fd, data, *pos) }))?;
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
        let fd = self.fd;
        ready!(self.ring().poll(ctx, true, |sqe| sqe.prep_close(fd)))?;
        Poll::Ready(Ok(()))
    }
}

impl<D: Drive> AsyncSeek for File<D> {
    fn poll_seek(mut self: Pin<&mut Self>, ctx: &mut Context, pos: io::SeekFrom)
        -> Poll<io::Result<u64>>
    {
        match pos {
            io::SeekFrom::Start(n)      => *self.as_mut().pos() = n as u64,
            io::SeekFrom::Current(n)    => {
                *self.as_mut().pos() += if n < 0 { n.abs() } else { n } as u64;
            }
            io::SeekFrom::End(n)        => {
                let end = ready!(self.as_mut().poll_file_size(ctx))?;
                *self.as_mut().pos() = end + if n < 0 { n.abs() } else { n} as u64;
            }
        }
        Poll::Ready(Ok(self.pos as u64))
    }
}

impl From<fs::File> for File {
    fn from(file: fs::File) -> File {
        File::run_on_driver(file, DemoDriver::default())
    }
}

impl<D: Drive> From<File<D>> for fs::File {
    fn from(mut file: File<D>) -> fs::File {
        unsafe {
            file.cancel();
            let file = ManuallyDrop::new(file);
            fs::File::from_raw_fd(file.fd)
        }
    }
}

impl<D: Drive> Drop for File<D> {
    fn drop(&mut self) {
        match self.active {
            Op::Nothing => unsafe { libc::close(self.fd); },
            _           => self.cancel(),
        }
    }
}

/// A future representing an opening file.
pub struct Open<D: Drive = DemoDriver<'static>>(Submission<OpenAt, D>);

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
pub struct Create<D: Drive = DemoDriver<'static>>(Submission<OpenAt, D>);

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

