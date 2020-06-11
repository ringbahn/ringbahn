use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::io;
use std::cmp;
use std::pin::Pin;
use std::mem::{self, MaybeUninit, ManuallyDrop};
use std::task::{Poll, Context};
use std::slice;

use futures_core::ready;
use crate::Cancellation;
use crate::drive::{Drive, ProvideBuffer};
use crate::Ring;

pub struct Buffer<D: Drive> {
    storage: Storage<D>,
    pos: u32,
    cap: u32,
}

enum Storage<D: Drive> {
    Read(ManuallyDrop<D::ReadBuf>),
    Write(ManuallyDrop<D::WriteBuf>),
    Statx(ManuallyDrop<Box<MaybeUninit<libc::statx>>>),
    Empty,
}

impl<D: Drive> Buffer<D> {
    pub fn new() -> Buffer<D> {
        Buffer {
            storage: Storage::Empty,
            pos: 0,
            cap: 0,
        }
    }

    pub fn buffered_from_read(&self) -> &[u8] {
        if let Storage::Read(buf) = &self.storage {
            let ptr: *mut u8 = buf.as_ref().as_ptr() as *mut u8;
            let cap = (self.cap - self.pos) as usize;
            unsafe { slice::from_raw_parts(ptr.offset(self.pos as isize), cap) }
        } else {
            &[]
        }
    }

    fn buffered_from_write(&self) -> &[u8] {
        if let Storage::Write(buf) = &self.storage {
            unsafe { mem::transmute(&buf.as_ref()[0..self.pos as usize]) }
        } else {
            &[]
        }
    }

    // invariant: if fill returns N, it must actually ahve filled read buf up to n bytes
    #[inline]
    pub unsafe fn fill_read_buf(
        &mut self,
        ctx: &mut Context<'_>,
        mut ring: Pin<&mut Ring<D>>,
        fill: impl FnOnce(Pin<&mut Ring<D>>, &mut Context<'_>, &mut D::ReadBuf) -> Poll<io::Result<u32>>
    ) -> Poll<io::Result<&[u8]>> {
        match &mut self.storage {
            Storage::Read(buf)  => {
                if self.pos >= self.cap {
                    self.cap = ready!(fill(ring, ctx, &mut *buf))?;
                    self.pos = 0;
                }
                Poll::Ready(Ok(self.buffered_from_read()))
            }
            Storage::Empty      => {
                ready!(self.alloc_read_buf(ctx, ring.as_mut()))?;
                if let Storage::Read(buf) = &mut self.storage {
                    if self.pos >= self.cap {
                        self.pos = ready!(fill(ring, ctx, &mut *buf))?;
                        self.cap = 0;
                    }
                    Poll::Ready(Ok(self.buffered_from_read()))
                } else { unreachable!() }
            }
            _                   => panic!("attempted to fill read buf while holding other buf"),
        }
    }

    #[inline]
    pub fn fill_write_buf(
        &mut self,
        ctx: &mut Context<'_>,
        mut ring: Pin<&mut Ring<D>>,
        fill: impl FnOnce(Pin<&mut Ring<D>>, &mut Context<'_>, &mut D::WriteBuf) -> Poll<io::Result<u32>>
    ) -> Poll<io::Result<&[u8]>> {
        match &mut self.storage {
            Storage::Write(buf)  => {
                if self.pos == 0 {
                    self.pos = ready!(fill(ring, ctx, &mut *buf))?;
                }
                Poll::Ready(Ok(self.buffered_from_write()))
            }
            Storage::Empty      => {
                ready!(self.alloc_write_buf(ctx, ring.as_mut()))?;
                if let Storage::Write(buf) = &mut self.storage {
                    if self.pos == 0 {
                        self.pos = ready!(fill(ring, ctx, &mut *buf))?;
                    }
                    Poll::Ready(Ok(self.buffered_from_write()))
                } else { unreachable!() }
            }
            _                   => panic!("attempted to fill read buf while holding other buf"),
        }
    }

    #[inline(always)]
    pub fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = unsafe { Pin::get_unchecked_mut(self) };
        this.pos = cmp::min(this.pos + amt as u32, this.cap);
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.pos = 0;
        self.cap = 0;
    }

    pub fn cancellation(&mut self) -> Cancellation {
        let cancellation = match &mut self.storage {
            Storage::Read(buf)      => unsafe { ProvideBuffer::cleanup(buf) },
            Storage::Write(buf)     => unsafe { ProvideBuffer::cleanup(buf) },
            Storage::Statx(statx)   => {
                unsafe fn callback(statx: *mut (), _: usize) {
                    dealloc(statx as *mut u8, Layout::new::<libc::statx>())
                }

                unsafe {
                    let statx = Box::into_raw(ManuallyDrop::take(statx));
                    Cancellation::new(statx as *mut (), 0, callback)
                }
            }
            Storage::Empty        => Cancellation::null(),
        };
        self.storage = Storage::Empty;
        cancellation
    }

    pub(crate) fn as_statx(&mut self) -> *mut libc::statx {
        match &mut self.storage {
            Storage::Statx(statx)   => statx.as_mut_ptr(),
            Storage::Empty          => {
                self.alloc_statx();
                if let Storage::Statx(statx) = &mut self.storage {
                    statx.as_mut_ptr()
                } else { unreachable!() }
            }
            _                       => panic!("accessed buffer as statx when storing something else"),
        }
    }

    fn alloc_read_buf(&mut self, ctx: &mut Context<'_>, ring: Pin<&mut Ring<D>>)
        -> Poll<io::Result<()>>
    {
        let buf = ready!(ring.driver_pinned().poll_provide_read_buf(ctx, 4096 * 2))?;
        self.storage = Storage::Read(ManuallyDrop::new(buf));
        Poll::Ready(Ok(()))
    }

    fn alloc_write_buf(&mut self, ctx: &mut Context<'_>, ring: Pin<&mut Ring<D>>)
        -> Poll<io::Result<()>>
    {
        let buf = ready!(ring.driver_pinned().poll_provide_write_buf(ctx, 4096 * 2))?;
        self.storage = Storage::Write(ManuallyDrop::new(buf));
        Poll::Ready(Ok(()))
    }

    fn alloc_statx(&mut self) {
        unsafe {
            let layout = Layout::new::<libc::statx>();
            let statx = alloc(layout);
            if statx.is_null() {
                handle_alloc_error(layout);
            }
            self.storage = Storage::Statx(ManuallyDrop::new(Box::from_raw(statx as *mut _)));
        }
    }
}

unsafe impl<D: Drive> Send for Buffer<D> where D::ReadBuf: Send, D::WriteBuf: Send { }
unsafe impl<D: Drive> Sync for Buffer<D> where D::ReadBuf: Sync, D::WriteBuf: Sync { }

impl<D: Drive> Drop for Buffer<D> {
    fn drop(&mut self) {
        unsafe {
            match &mut self.storage {
                Storage::Read(buf)      => ManuallyDrop::drop(buf),
                Storage::Write(buf)     => ManuallyDrop::drop(buf),
                Storage::Statx(statx)   => ManuallyDrop::drop(statx),
                Storage::Empty          => return,
            }
        }
    }
}
