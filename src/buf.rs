use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::io;
use std::cmp;
use std::mem;
use std::ptr::NonNull;
use std::slice;
use std::task::Poll;

use futures_core::ready;
use crate::event::Cancellation;

pub struct Buffer {
    data: NonNull<()>,
    storage: Storage,
    capacity: u32,
    pos: u32,
    cap: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Storage {
    Nothing = 0,
    Buffer,
    Statx,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            data: NonNull::dangling(),
            storage: Storage::Nothing,
            capacity: 4096 * 2,
            pos: 0,
            cap: 0,
        }
    }

    #[inline(always)]
    pub fn buffered_from_read(&self) -> &[u8] {
        if self.storage == Storage::Buffer {
            unsafe {
                let data: *mut u8 = self.data.cast().as_ptr();
                let cap = self.cap - self.pos;
                slice::from_raw_parts(data.offset(self.pos as isize), cap as usize)
            }
        } else {
            &[]
        }
    }

    #[inline]
    pub fn fill_buf(&mut self, fill: impl FnOnce(&mut [u8]) -> Poll<io::Result<u32>>)
        -> Poll<io::Result<&[u8]>>
    {
        match self.storage {
            Storage::Buffer => {
                if self.pos >= self.cap {
                    let buf = unsafe {
                        slice::from_raw_parts_mut(self.data.cast().as_ptr(), self.capacity as usize)
                    };
                    self.cap = ready!(fill(buf))?;
                    self.pos = 0;
                }

                Poll::Ready(Ok(self.buffered_from_read()))
            }
            Storage::Nothing => {
                self.cap = ready!(fill(self.alloc_buf()))?;
                Poll::Ready(Ok(self.buffered_from_read()))
            }
            _               => panic!("attempted to fill buf while not holding buffer"),
        }
    }

    #[inline(always)]
    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt as u32, self.cap);
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.pos = 0;
        self.cap = 0;
    }

    pub fn cancellation(&mut self) -> Cancellation {
        match self.storage {
            Storage::Buffer     => {
                self.clear();
                self.storage = Storage::Nothing;
                let data = mem::replace(&mut self.data, NonNull::dangling());
                unsafe { Cancellation::buffer(data.cast().as_ptr(), self.capacity as usize) }
            }
            Storage::Statx      => {
                unsafe fn callback(statx: *mut (), _: usize) {
                    dealloc(statx as *mut u8, Layout::new::<libc::statx>())
                }

                self.storage = Storage::Nothing;
                let data = mem::replace(&mut self.data, NonNull::dangling());
                Cancellation::new(data.cast().as_ptr(), 0, callback)
            }
            Storage::Nothing    => Cancellation::null(),
        }
    }

    pub(crate) fn as_statx(&mut self) -> *mut libc::statx {
        match self.storage {
            Storage::Statx      => self.data.cast().as_ptr(),
            Storage::Nothing    => self.alloc_statx(),
            _                   => panic!("accessed buffer as statx when storing something else"),
        }
    }

    fn alloc_buf(&mut self) -> &mut [u8] {
        self.storage = Storage::Buffer;
        self.alloc();
        unsafe {
            slice::from_raw_parts_mut(self.data.cast().as_ptr(), self.capacity as usize)
        }
    }

    fn alloc_statx(&mut self) -> &mut libc::statx {
        self.storage = Storage::Statx;
        self.alloc();
        unsafe {
            &mut *self.data.cast().as_ptr()
        }
    }

    fn alloc(&mut self) {
        unsafe {
            let layout = self.layout().unwrap();
            let ptr = alloc(layout);
            if ptr.is_null() {
                self.storage = Storage::Nothing;
                handle_alloc_error(layout)
            }
            self.data = NonNull::new_unchecked(ptr).cast();
        }
    }

    #[inline(always)]
    fn layout(&self) -> Option<Layout> {
        match self.storage {
            Storage::Statx      => Some(Layout::new::<libc::statx>()),
            Storage::Buffer     => Some(Layout::array::<u8>(self.capacity as usize).unwrap()),
            Storage::Nothing    => None,
        }
    }
}

unsafe impl Send for Buffer { }
unsafe impl Sync for Buffer { }

impl Drop for Buffer {
    fn drop(&mut self) {
        if let Some(layout) = self.layout() {
            unsafe {
                dealloc(self.data.cast().as_ptr(), layout);
            }
        }
    }
}
