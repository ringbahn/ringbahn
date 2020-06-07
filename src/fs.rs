use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::cmp;
use std::fs;
use std::future::Future;
use std::io;
use std::mem::{self, ManuallyDrop};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;
use std::pin::Pin;
use std::ptr;
use std::slice;
use std::task::{Context, Poll};

use futures_core::ready;
use futures_io::{AsyncRead, AsyncBufRead, AsyncWrite, AsyncSeek};

use crate::completion::Completion;
use crate::drive::Completion as ExternalCompletion;
use crate::drive::Drive;
use crate::drive::demo::DemoDriver;
use crate::event::{OpenAt, Cancellation};
use crate::Submission;

use State::*;

pub struct File<D: Drive = DemoDriver<'static>> {
    state: State,
    current: Current,
    fd: RawFd,
    completion: Option<Completion>,
    buf: Buffer,
    pos: usize,
    driver: D,
}

#[derive(Debug, Eq, PartialEq)]
enum State {
    Inert = 0,
    Prepared,
    Submitted,
    Lost,
}

#[derive(Eq, PartialEq)]
enum Current {
    Nothing = 0,
    Read,
    Write,
    Close,
}

pub struct Open<D: Drive = DemoDriver<'static>>(Submission<OpenAt, D>);

impl<D: Drive> Future for Open<D> {
    type Output = io::Result<File<D>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<File<D>>> {
        let (_, driver, result) = ready!(self.inner().poll(ctx));
        let fd = result? as i32;
        Poll::Ready(Ok(File::from_fd(fd, driver)))
    }
}

impl<D: Drive> Open<D> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut Submission<OpenAt, D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.0) }
    }
}

pub struct Create<D: Drive = DemoDriver<'static>>(Submission<OpenAt, D>);

impl<D: Drive> Create<D> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut Submission<OpenAt, D>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.0) }
    }
}

impl<D: Drive> Future for Create<D> {
    type Output = io::Result<File<D>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<File<D>>> {
        let (_, driver, result) = ready!(self.inner().poll(ctx));
        let fd = result? as i32;
        Poll::Ready(Ok(File::from_fd(fd, driver)))
    }
}

impl From<fs::File> for File {
    fn from(file: fs::File) -> File {
        File::run_on_driver(file, DemoDriver::default())
    }
}

impl File {
    pub fn open(path: impl AsRef<Path>) -> Open {
        File::open_on_driver(path, DemoDriver::default())
    }

    pub fn create(path: impl AsRef<Path>) -> Create {
        File::create_on_driver(path, DemoDriver::default())
    }
}

impl<D: Drive> File<D> {
    pub fn open_on_driver(path: impl AsRef<Path>, driver: D) -> Open<D> {
        let flags = libc::O_CLOEXEC | libc::O_RDONLY;
        let event = OpenAt::new(path, libc::AT_FDCWD, flags, 0o666);
        Open(Submission::new(event, driver))
    }

    pub fn create_on_driver(path: impl AsRef<Path>, driver: D) -> Create<D> {
        let flags = libc::O_CLOEXEC | libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
        let event = OpenAt::new(path, libc::AT_FDCWD, flags, 0o666);
        Create(Submission::new(event, driver))
    }

    pub fn run_on_driver(file: fs::File, driver: D) -> File<D> {
        let file = ManuallyDrop::new(file);
        File::from_fd(file.as_raw_fd(), driver)
    }

    fn from_fd(fd: RawFd, driver: D) -> File<D> {
        File {
            state: Inert,
            current: Current::Nothing,
            buf: Buffer::new(),
            completion: None,
            pos: 0,
            fd, driver
        }
    }

    pub fn read_buffered(&self) -> &[u8] {
        if self.current == Current::Read { 
            todo!()
        } else { &[] }
    }

    pub fn write_buffered(&self) -> &[u8] {
        if self.current == Current::Write { 
            todo!()
        } else { &[] }
    }

    unsafe fn poll_read_op(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let pos = self.pos;
        let fd = self.fd;
        let n = ready!(self.as_mut().poll(ctx, |sqe, buf| {
            sqe.prep_read(fd, buf.read_buf(), pos);
        }))?;
        *self.pos() += n;
        Poll::Ready(Ok(n))
    }

    unsafe fn poll_write_op(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let pos = self.pos;
        let fd = self.fd;
        let n = ready!(self.as_mut().poll(ctx, |sqe, buf| {
            sqe.prep_write(fd, buf.write_buf(), pos);
        }))?;
        *self.pos() += n;
        Poll::Ready(Ok(n))
    }

    unsafe fn poll_close_op(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let fd = self.fd;
        self.poll(ctx, |sqe, _| sqe.prep_close(fd))
    }

    #[inline]
    unsafe fn poll(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(&mut iou::SubmissionQueueEvent<'_>, &mut Buffer),
    ) -> Poll<io::Result<usize>> {
        match self.state {
            Inert       => {
                ready!(self.as_mut().try_prepare(ctx, prepare));
                ready!(self.as_mut().try_submit(ctx));
                Poll::Pending
            }
            Prepared    => {
                match self.as_mut().try_complete(ctx) {
                    ready @ Poll::Ready(..) => ready,
                    Poll::Pending           => {
                        ready!(self.as_mut().try_submit(ctx));
                        Poll::Pending
                    }
                }
            }
            Submitted   => self.try_complete(ctx),
            Lost        => panic!("File in a bad state; driver is faulty"),
        }
    }

    #[inline]
    unsafe fn try_prepare(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        prepare: impl FnOnce(&mut iou::SubmissionQueueEvent<'_>, &mut Buffer)
    ) -> Poll<()> {
        let this = Pin::get_unchecked_mut(self);
        let driver = Pin::new_unchecked(&mut this.driver);
        let state = &mut this.state;
        let buf = &mut this.buf;
        let completion = ready!(driver.poll_prepare(ctx, |mut sqe, ctx| {
            *state = Lost;
            prepare(&mut sqe, buf);
            let completion = Completion::new(ctx.waker().clone());
            sqe.set_user_data(completion.addr());
            ExternalCompletion::new(completion, ctx)
        }));
        *state = Prepared;
        this.completion = Some(completion.real);
        Poll::Ready(())
    }

    #[inline]
    unsafe fn try_submit(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        // TODO figure out how to handle this result
        let _ = ready!(self.as_mut().driver().poll_submit(ctx, true));
        *self.state() = Submitted;
        Poll::Ready(())
    }

    #[inline]
    unsafe fn try_complete(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if let Some(result) = self.completion.as_ref().and_then(|c| c.check()) {
            *self.as_mut().state() = Inert;
            self.as_mut().completion().take().unwrap().deallocate();
            Poll::Ready(result)
        } else {
            if let Some(completion) = &self.completion {
                completion.set_waker(ctx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel(&mut self) {
        unsafe {
            match self.current {
                Current::Read | Current::Write    => {
                    let mut cancellation = self.buf.cancellation();
                    if let Some(completion) = self.completion.take() {
                        completion.cancel(cancellation);
                    } else {
                        cancellation.cancel();
                    }
                    self.current = Current::Nothing;
                }
                Current::Close                   => {
                    if let Some(completion) = self.completion.take() {
                        completion.cancel(Cancellation::null());
                    }
                }
                Current::Nothing                 => { }
            }
        }
    }

    #[inline]
    fn driver(self: Pin<&mut Self>) -> Pin<&mut D> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.driver) }
    }

    #[inline]
    fn state(self: Pin<&mut Self>) -> Pin<&mut State> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.state) }
    }

    #[inline]
    fn completion(self: Pin<&mut Self>) -> Pin<&mut Option<Completion>> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.completion) }
    }

    #[inline]
    fn buf(self: Pin<&mut Self>) -> Pin<&mut Buffer> {
        unsafe { Pin::map_unchecked_mut(self, |this| &mut this.buf) }
    }

    #[inline]
    fn pos(self: Pin<&mut Self>) -> Pin<&mut usize> {
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
    fn poll_fill_buf(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        unsafe {
            let this: &mut File<D> = Pin::get_unchecked_mut(self);

            if !matches!(this.current, Current::Read | Current::Nothing) {
                this.cancel();
            }

            this.current = Current::Read;
            if this.buf.consumed >= this.buf.read {
                this.buf.read = ready!(Pin::new_unchecked(&mut *this).poll_read_op(ctx))? as u32;
                this.buf.consumed = 0;
            }
            let consumed = this.buf.consumed as usize;
            let read = this.buf.read as usize;
            let slice = &this.buf.data()[consumed..read];
            Poll::Ready(Ok(slice))
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.buf().consume(amt);
    }
}

impl<D: Drive> AsyncWrite for File<D> {
    fn poll_write(self: Pin<&mut Self>, ctx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        unsafe {
            let this: &mut File<D> = Pin::get_unchecked_mut(self);

            if !matches!(this.current, Current::Write | Current::Nothing) {
                this.cancel();
            }

            this.current = Current::Write;
            if this.buf.written == 0 {
                this.buf.written = io::Write::write(&mut this.buf.data_mut(), buf).unwrap() as u32;
            }

            let result = ready!(Pin::new_unchecked(&mut *this).poll_write_op(ctx));
            this.buf.written = 0;
            Poll::Ready(result)
        }
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_write(ctx, &[]))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        unsafe {
            let this: &mut File<D> = Pin::get_unchecked_mut(self);

            if !matches!(this.current, Current::Close | Current::Nothing) {
                this.cancel();
            }

            this.current = Current::Close;
            ready!(Pin::new_unchecked(this).poll_close_op(ctx))?;
            Poll::Ready(Ok(()))
        }

    }
}

impl<D: Drive> AsyncSeek for File<D> {
    fn poll_seek(mut self: Pin<&mut Self>, _: &mut Context, pos: io::SeekFrom)
        -> Poll<io::Result<u64>>
    {
        match pos {
            io::SeekFrom::Start(n)      => *self.as_mut().pos() = n as usize,
            io::SeekFrom::Current(n)    => {
                *self.as_mut().pos() += if n < 0 { n.abs() } else { n } as usize;
            }
            io::SeekFrom::End(_)        => {
                const MSG: &str = "cannot seek to end of io-uring file";
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, MSG)))
            }
        }
        Poll::Ready(Ok(self.pos as u64))
    }
}

impl<D: Drive> Drop for File<D> {
    fn drop(&mut self) {
        if self.current == Current::Nothing {
            unsafe {
                libc::close(self.fd);
            }
        } else {
            self.cancel();
        }
    }
}

struct Buffer {
    data: *mut u8,
    capacity: u32,
    consumed: u32,
    read: u32,
    written: u32,
}

impl Buffer {
    fn new() -> Buffer {
        let capacity = 4096 * 2;
        let data = ptr::null_mut();

        Buffer {
            data, capacity,
            consumed: 0,
            read: 0,
            written: 0,
        }
    }

    fn read_buf(&mut self) -> &mut [u8] {
        &mut self.data_mut()[..]
    }

    fn write_buf(&mut self) -> &mut [u8] {
        let written = self.written as usize;
        &mut self.data_mut()[..written]
    }

    fn consume(&mut self, amt: usize) {
        self.consumed = cmp::min(self.consumed + amt as u32, self.read);
    }

    fn data(&mut self) -> &[u8] {
        let data = self.lazy_alloc();
        unsafe { slice::from_raw_parts(data, self.capacity as usize) }
    }

    fn data_mut(&mut self) -> &mut [u8] {
        let data = self.lazy_alloc();
        unsafe { slice::from_raw_parts_mut(data, self.capacity as usize) }
    }

    fn cancellation(&mut self) -> Cancellation {
        let data = mem::replace(&mut self.data, ptr::null_mut());
        unsafe { Cancellation::buffer(data, self.capacity as usize) }
    }

    #[inline(always)]
    fn lazy_alloc(&mut self) -> *mut u8 {
        if self.data == ptr::null_mut() {
            let layout = Layout::array::<u8>(self.capacity as usize).unwrap();
            let ptr = unsafe { alloc(layout) };
            if ptr == ptr::null_mut() {
                handle_alloc_error(layout);
            }
            self.data = ptr;
        }

        self.data
    }
}

unsafe impl Send for Buffer { }
unsafe impl Sync for Buffer { }

impl Drop for Buffer {
    fn drop(&mut self) {
        if self.data != ptr::null_mut() {
            unsafe {
                dealloc(self.data, Layout::array::<u8>(self.capacity as usize).unwrap());
            }
        }
    }
}
