use futures_core::ready;

use std::cmp;
use std::io;
use std::mem;
use std::os::unix::io::RawFd;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll};

use crate::completion::Completion;
use crate::event::Cancellation;
use crate::drive::{Drive, Completion as ExternalCompletion};

use State::*;

pub struct Engine {
    state: State,
    read_buf: Buffer,
    write_buf: Buffer,
    completion: Completion,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum State {
    Inert,
    ReadPrepared,
    ReadSubmitted,
    WriteBuffered,
    WritePrepared,
    WriteSubmitted,
    Lost,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            state: Inert,
            read_buf: Buffer::new(),
            write_buf: Buffer::new(),
            completion: Completion::dangling(),
        }
    }

    pub fn poll_fill_read_buf<D: Drive>(
        &mut self,
        ctx: &mut Context<'_>,
        driver: Pin<&mut D>,
        fd: RawFd,
    ) -> Poll<io::Result<&[u8]>>
    {
        if self.read_buf.pos >= self.read_buf.cap {
            self.read_buf.cap = ready!(self.poll_read(ctx, driver, fd))?;
            self.read_buf.pos = 0;
        }

        Poll::Ready(Ok(&self.read_buf.active()))
    }

    #[inline(always)]
    pub fn consume(&mut self, amt: usize) {
        self.read_buf.consume(amt);
    }

    pub fn poll_flush_write_buf<D: Drive>(
        &mut self,
        ctx: &mut Context<'_>,
        mut driver: Pin<&mut D>,
        fd: RawFd,
    )-> Poll<io::Result<()>> {
        let result = loop {
            if self.write_buf.pos >= self.write_buf.cap {
                break Ok(());
            }

            match ready!(self.poll_write(ctx, driver.as_mut(), fd, &[])) {
                Ok(0)   => break Err(io::Error::new(io::ErrorKind::WriteZero, "write failed")),
                Ok(n)   => self.write_buf.pos += n,
                Err(e)  => break Err(e),
            }
        };
        self.write_buf.copy_remaining();
        Poll::Ready(result)
    }

    #[inline(always)]
    pub fn poll_write<D: Drive>(
        &mut self,
        ctx: &mut Context<'_>,
        mut driver: Pin<&mut D>,
        fd: RawFd,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        unsafe {
            match self.state {
                WriteSubmitted  => self.try_complete(ctx),
                WritePrepared   => {
                    match self.try_complete(ctx) {
                        ready @ Poll::Ready(..) => ready,
                        Poll::Pending           => {
                            ready!(self.try_submit(ctx, driver));
                            self.state = WriteSubmitted;
                            Poll::Pending
                        }
                    }
                }
                Lost            => panic!("Ring in a bad state; driver is faulty"),
                _               => {
                    if !matches!(self.state, WriteBuffered) {
                        self.cancel();
                        self.write_buf.write(buf);
                        self.state = WriteBuffered;
                    }
                    self.completion = ready!(driver.as_mut().poll_prepare(ctx, |sqe, ctx| {
                        prepare_write(sqe, ctx, fd, &mut self.write_buf.active(), &mut self.state)
                    })).real;
                    self.state = WritePrepared;
                    ready!(self.try_submit(ctx, driver));
                    self.state = WriteSubmitted;
                    Poll::Pending
                }
            }
        }
    }


    #[inline(always)]
    pub fn cancel(&mut self) {
        unsafe {
            match self.state {
                ReadPrepared | ReadSubmitted    => {
                    let data = self.read_buf.buf.as_mut_ptr();
                    let len = self.read_buf.buf.len();
                    self.completion.cancel(Cancellation::buffer(data, len));
                    ptr::write(&mut self.read_buf, Buffer::new());
                    self.state = Inert;
                }
                WritePrepared | WriteSubmitted  => {
                    let data = self.write_buf.buf.as_mut_ptr();
                    let len = self.write_buf.buf.len();
                    self.completion.cancel(Cancellation::buffer(data, len));
                    ptr::write(&mut self.write_buf, Buffer::new());
                    self.state = Inert;
                }
                WriteBuffered                   => {
                    self.write_buf.pos = 0;
                    self.write_buf.cap = 0;
                    self.state = Inert;
                }
                Inert | Lost                    => { }
            }
        }
    }

    #[inline(always)]
    fn poll_read<D: Drive>(
        &mut self,
        ctx: &mut Context<'_>,
        mut driver: Pin<&mut D>,
        fd: RawFd,
    ) -> Poll<io::Result<usize>> {
        unsafe {
            match self.state {
                ReadSubmitted   => self.try_complete(ctx),
                ReadPrepared    => {
                    match self.try_complete(ctx) {
                        ready @ Poll::Ready(..) => ready,
                        Poll::Pending           => {
                            ready!(self.try_submit(ctx, driver));
                            self.state = ReadSubmitted;
                            Poll::Pending
                        }
                    }
                }
                Lost            => panic!("Ring in a bad state; driver is faulty"),
                _ => {
                    self.cancel();
                    self.completion = ready!(driver.as_mut().poll_prepare(ctx, |sqe, ctx| {
                        prepare_read(sqe, ctx, fd, &mut self.read_buf.buf[..], &mut self.state)
                    })).real;
                    self.state = ReadPrepared;
                    ready!(self.try_submit(ctx, driver));
                    self.state = ReadSubmitted;
                    Poll::Pending
                }
            }
        }
    }

    #[inline(always)]
    unsafe fn try_submit<D: Drive>(&mut self, ctx: &mut Context<'_>, driver: Pin<&mut D>)
        -> Poll<()>
    {
        // TODO figure out how to handle this result
        let _ = ready!(driver.poll_submit(ctx, true));
        Poll::Pending
    }

    #[inline(always)]
    unsafe fn try_complete(&mut self, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if let Some(result) = self.completion.check() {
            self.state = Inert;
            self.completion.deallocate();
            Poll::Ready(result)
        } else {
            self.completion.set_waker(ctx.waker().clone());
            Poll::Pending
        }
    }

}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel();
    }
}

unsafe fn prepare_read<'cx>(
    sqe: iou::SubmissionQueueEvent<'_>,
    ctx: &mut Context<'cx>, 
    fd: RawFd,
    buf: &mut [u8],
    state: &mut State,
) -> ExternalCompletion<'cx> {
    let mut sqe = SubmissionCleaner(sqe);
    sqe.0.prep_read(fd, buf, 0);

    let completion = Completion::new(ctx.waker().clone());
    sqe.0.set_user_data(completion.addr());
    *state = Lost;
    mem::forget(sqe);
    ExternalCompletion::new(completion, ctx)
}

unsafe fn prepare_write<'cx>(
    sqe: iou::SubmissionQueueEvent<'_>,
    ctx: &mut Context<'cx>, 
    fd: RawFd,
    buf: &[u8],
    state: &mut State,
) -> ExternalCompletion<'cx> {
    let mut sqe = SubmissionCleaner(sqe);
    sqe.0.prep_write(fd, buf, 0);

    let completion = Completion::new(ctx.waker().clone());
    sqe.0.set_user_data(completion.addr());
    *state = Lost;
    mem::forget(sqe);
    ExternalCompletion::new(completion, ctx)
}

// Use the SubmissionCleaner guard to clear the submission of any data
// in case the Event::prepare method panics
struct SubmissionCleaner<'a>(iou::SubmissionQueueEvent<'a>);

impl<'a> Drop for SubmissionCleaner<'a> {
    fn drop(&mut self) {
        unsafe {
            self.0.prep_nop();
            self.0.set_user_data(0);
        }
    }
}

struct Buffer {
    buf: Box<[u8]>,
    pos: usize,
    cap: usize,
}

impl Buffer {
    fn new() -> Buffer {
        Buffer {
            buf: vec![0; 1024 * 8].into_boxed_slice(),
            pos: 0,
            cap: 0,
        }
    }

    #[inline(always)]
    fn write(&mut self, buf: &[u8]) {
        let len = io::Write::write(&mut &mut self.buf[..], buf).unwrap();
        self.pos = 0;
        self.cap = len;
    }

    #[inline(always)]
    fn active(&self) -> &[u8] {
        &self.buf[self.pos..self.cap]
    }

    #[inline(always)]
    fn copy_remaining(&mut self) {
        self.buf.copy_within(self.pos..self.cap, 0);
        self.cap -= self.pos;
        self.pos = 0;
    }

    #[inline(always)]
    fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.cap);
    }
}
