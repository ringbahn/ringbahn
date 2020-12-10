//! Interact with the file system using io-uring

use std::fmt;
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
use crate::ring::{Cancellation, Ring};
use crate::Submission;

type FileBuf = Either<Buffer, Box<libc::statx>>;

/// A file handle that runs on io-uring
pub struct File<D: Drive = DemoDriver> {
    ring: Ring<D>,
    fd: RawFd,
    active: Op,
    buf: FileBuf,
    pos: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Op {
    Read,
    Write,
    Close,
    Nothing,
    Statx,
    Closed,
    SyncAll,
    SyncData,
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

    /// Synchronizes OS-internal buffered contents and metadata to disk.
    pub fn sync_all(&mut self) -> SyncAll<'_, D>
    where
        D: Unpin,
    {
        Pin::new(self).sync_all_pinned()
    }

    fn sync_all_pinned(self: Pin<&mut Self>) -> SyncAll<'_, D> {
        SyncAll { file: self }
    }

    fn poll_sync_all(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.as_mut().guard_op(Op::SyncAll);
        let fd = self.fd;
        ready!(self.as_mut().ring().poll(ctx, 1, |sqs| {
            let mut sqe = sqs.single().unwrap();
            unsafe {
                sqe.prep_fsync(fd, iou::sqe::FsyncFlags::empty());
            }
            sqe
        }))?;
        self.confirm_close();
        Poll::Ready(Ok(()))
    }

    /// This function is similar to [`sync_all`], except that it may not
    /// synchronize file metadata to the filesystem.
    pub fn sync_data(&mut self) -> SyncData<'_, D>
    where
        D: Unpin,
    {
        Pin::new(self).sync_data_pinned()
    }

    fn sync_data_pinned(self: Pin<&mut Self>) -> SyncData<'_, D> {
        SyncData { file: self }
    }

    fn poll_sync_data(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.as_mut().guard_op(Op::SyncData);
        let fd = self.fd;
        ready!(self.as_mut().ring().poll(ctx, 1, |sqs| {
            let mut sqe = sqs.single().unwrap();
            unsafe {
                sqe.prep_fsync(fd, iou::sqe::FsyncFlags::FSYNC_DATASYNC);
            }
            sqe
        }))?;
        self.confirm_close();
        Poll::Ready(Ok(()))
    }

    /// Returns the file type for this metadata.
    pub fn metadata(&mut self) -> MetadataFut<'_, D>
    where
        D: Unpin,
    {
        Pin::new(self).metadata_pinned()
    }

    fn metadata_pinned(self: Pin<&mut Self>) -> MetadataFut<'_, D>
    where
        D: Unpin,
    {
        MetadataFut { file: self }
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
        let (ring, buf, .., active) = self.split();
        if *active == Op::Closed {
            panic!("Attempted to perform IO on a closed File");
        } else if *active != Op::Nothing && *active != op {
            let new_buf = Either::Left(Buffer::default());
            ring.cancel_pinned(Cancellation::from(mem::replace(buf, new_buf)));
        }
        *active = op;
    }

    fn cancel(&mut self) {
        self.active = Op::Nothing;
        let new_buf = Either::Left(Buffer::default());
        self.ring
            .cancel(Cancellation::from(mem::replace(&mut self.buf, new_buf)));
    }

    fn poll_statx(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<io::Result<libc::statx>> {
        static EMPTY: libc::c_char = 0;
        use std::ffi::CStr;

        self.as_mut().guard_op(Op::Statx);
        let fd = self.fd;
        let (ring, statx, ..) = self.split_with_statx();
        let flags = iou::sqe::StatxFlags::AT_EMPTY_PATH;
        let mask = iou::sqe::StatxMode::STATX_SIZE;
        ready!(ring.poll(ctx, 1, |sqs| {
            let mut sqe = sqs.single().unwrap();
            unsafe {
                sqe.prep_statx(fd, CStr::from_ptr(&EMPTY), flags, mask, statx);
            }
            sqe
        }))?;
        Poll::Ready(Ok(*statx))
    }

    fn poll_file_size(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        let statx = ready!(self.poll_statx(ctx))?;
        Poll::Ready(Ok(statx.stx_size))
    }

    fn poll_metadata(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<Metadata>> {
        let statx = ready!(self.poll_statx(ctx))?;
        Poll::Ready(Ok(Metadata { stat: statx }))
    }

    #[inline(always)]
    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut FileBuf, &mut u64, &mut Op) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (
                Pin::new_unchecked(&mut this.ring),
                &mut this.buf,
                &mut this.pos,
                &mut this.active,
            )
        }
    }

    #[inline(always)]
    fn split_with_buf(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Buffer, &mut u64, &mut Op) {
        let (ring, buf, pos, active) = self.split();
        let buf = buf.as_mut().unwrap_left();
        (ring, buf, pos, active)
    }

    #[inline(always)]
    fn split_with_statx(
        self: Pin<&mut Self>,
    ) -> (Pin<&mut Ring<D>>, &mut libc::statx, &mut u64, &mut Op) {
        let (ring, buf, pos, active) = self.split();
        if buf.is_left() {
            *buf = Either::Right(Box::new(unsafe { mem::zeroed() }));
        }
        let statx = buf.as_mut().unwrap_right();
        (ring, statx, pos, active)
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

    fn confirm_close(self: Pin<&mut Self>) {
        *self.split().3 = Op::Closed;
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
        let fd = self.fd;
        let (ring, buf, pos, ..) = self.split_with_buf();
        buf.fill_buf(|buf| {
            let n = ready!(ring.poll(ctx, 1, |sqs| {
                let mut sqe = sqs.single().unwrap();
                unsafe {
                    sqe.prep_read(fd, buf, *pos);
                }
                sqe
            }))?;
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
        let fd = self.fd;
        let (ring, buf, pos, ..) = self.split_with_buf();
        let data =
            ready!(buf.fill_buf(|mut buf| {
                Poll::Ready(Ok(io::Write::write(&mut buf, slice)? as u32))
            }))?;
        let n = ready!(ring.poll(ctx, 1, |sqs| {
            let mut sqe = sqs.single().unwrap();
            unsafe {
                sqe.prep_write(fd, data, *pos);
            }
            sqe
        }))?;
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
        ready!(self.as_mut().ring().poll(ctx, 1, |sqs| {
            let mut sqe = sqs.single().unwrap();
            unsafe {
                sqe.prep_close(fd);
            }
            sqe
        }))?;
        self.confirm_close();
        Poll::Ready(Ok(()))
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

impl<D: Drive> From<File<D>> for fs::File {
    fn from(mut file: File<D>) -> fs::File {
        file.cancel();
        let file = ManuallyDrop::new(file);
        unsafe { fs::File::from_raw_fd(file.fd) }
    }
}

impl<D: Drive> Drop for File<D> {
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

/// A future representing an syncing a file.
pub struct SyncAll<'a, D: Drive> {
    file: Pin<&'a mut File<D>>,
}

impl<'a, D: Drive + Clone> Future for SyncAll<'a, D> {
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.file.as_mut().poll_sync_all(ctx)
    }
}

/// A future representing an syncing a file.
pub struct SyncData<'a, D: Drive> {
    file: Pin<&'a mut File<D>>,
}

impl<'a, D: Drive + Clone> Future for SyncData<'a, D> {
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.file.as_mut().poll_sync_data(ctx)
    }
}

/// A future representing an syncing a file.
pub struct MetadataFut<'a, D: Drive> {
    file: Pin<&'a mut File<D>>,
}

impl<'a, D: Drive + Clone> Future for MetadataFut<'a, D> {
    type Output = io::Result<Metadata>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<Metadata>> {
        self.file.as_mut().poll_metadata(ctx)
    }
}

/// Metadata information about a file.
#[derive(Clone)]
pub struct Metadata {
    stat: libc::statx,
}

impl Metadata {
    /// Returns the file type for this metadata.
    pub fn file_type(&self) -> FileType {
        FileType(self.stat.stx_mode as libc::mode_t)
    }

    /// Returns `true` if this metadata is for a directory. The
    /// result is mutually exclusive to the result of
    /// [`Metadata::is_file`], and will be false for symlink metadata
    /// obtained from [`symlink_metadata`].
    pub fn is_dir(&self) -> bool {
        self.file_type().is_dir()
    }

    /// Returns `true` if this metadata is for a regular file. The
    /// result is mutually exclusive to the result of
    /// [`Metadata::is_dir`], and will be false for symlink metadata
    /// obtained from [`symlink_metadata`].
    pub fn is_file(&self) -> bool {
        self.file_type().is_file()
    }

    /// Returns the size of the file, in bytes, this metadata is for.
    pub fn len(&self) -> u64 {
        self.stat.stx_size
    }

    /// Returns the permissions of the file this metadata is for.
    pub fn permissions(&self) -> Permissions {
        Permissions(self.stat.stx_mode as libc::mode_t)
    }

    /// Returns the last modification time listed in this metadata.
    pub fn modified(&self) -> io::Result<libc::statx_timestamp> {
        Ok(self.stat.stx_mtime)
    }

    /// Returns the last access time of this metadata.
    pub fn accessed(&self) -> io::Result<libc::statx_timestamp> {
        Ok(self.stat.stx_atime)
    }

    /// Returns the creation time listed in this metadata.
    pub fn created(&self) -> io::Result<libc::statx_timestamp> {
        Ok(self.stat.stx_btime)
    }
}

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metadata")
            .field("file_type", &self.file_type())
            .field("is_dir", &self.is_dir())
            .field("is_file", &self.is_file())
            .field("permissions", &self.permissions())
            .field("modified", &self.modified())
            .field("accessed", &self.accessed())
            .field("created", &self.created())
            .finish()
    }
}

/// Representation of the various permissions on a file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Permissions(libc::mode_t);

/// A structure representing a type of file with accessors for each file type.
/// It is returned by [`Metadata::file_type`] method.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FileType(libc::mode_t);

impl Permissions {
    /// Returns `true` if these permissions describe a readonly (unwritable) file.
    pub fn readonly(&self) -> bool {
        self.0 & 0o222 == 0
    }

    /// Modifies the readonly flag for this set of permissions. If the
    /// `readonly` argument is `true`, using the resulting `Permission` will
    /// update file permissions to forbid writing. Conversely, if it's `false`,
    /// using the resulting `Permission` will update file permissions to allow
    /// writing.
    ///
    /// This operation does **not** modify the filesystem. To modify the
    /// filesystem use the [`set_permissions`] function.
    pub fn set_readonly(&mut self, readonly: bool) {
        if readonly {
            // remove write permission for all classes; equivalent to `chmod a-w <file>`
            self.0 &= !0o222;
        } else {
            // add write permission for all classes; equivalent to `chmod a+w <file>`
            self.0 |= 0o222;
        }
    }
}

impl FileType {
    /// Tests whether this file type represents a directory. The
    /// result is mutually exclusive to the results of
    /// [`is_file`] and [`is_symlink`]; only zero or one of these
    /// tests may pass.
    pub fn is_dir(&self) -> bool {
        self.is(libc::S_IFDIR)
    }

    /// Tests whether this file type represents a regular file.
    /// The result is  mutually exclusive to the results of
    /// [`is_dir`] and [`is_symlink`]; only zero or one of these
    /// tests may pass.
    pub fn is_file(&self) -> bool {
        self.is(libc::S_IFREG)
    }

    /// Tests whether this file type represents a symbolic link.
    /// The result is mutually exclusive to the results of
    /// [`is_dir`] and [`is_file`]; only zero or one of these
    /// tests may pass.
    pub fn is_symlink(&self) -> bool {
        self.is(libc::S_IFLNK)
    }

    fn is(&self, mode: libc::mode_t) -> bool {
        self.0 & libc::S_IFMT == mode
    }
}
