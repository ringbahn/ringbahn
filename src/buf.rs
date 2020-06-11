use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::io;
use std::cmp;
use std::mem;
use std::ptr;
use std::slice;
use std::task::Poll;

use futures_core::ready;
use crate::event::Cancellation;

pub struct Buffer {
    data: *mut u8,
    capacity: u32,
    pos: u32,
    cap: u32,
}

impl Buffer {
    pub fn new() -> Buffer {
        let capacity = 4096 * 2;
        let data = ptr::null_mut();

        Buffer {
            data, capacity,
            pos: 0,
            cap: 0,
        }
    }

    #[inline(always)]
    pub fn buffered_from_read(&self) -> &[u8] {
        if self.data == ptr::null_mut() {
            &[]
        } else {
            unsafe {
                let cap = self.cap - self.pos;
                slice::from_raw_parts(self.data.offset(self.pos as isize), cap as usize)
            }
        }
    }

    #[inline(always)]
    pub fn fill_buf(&mut self, fill: impl FnOnce(&mut [u8]) -> Poll<io::Result<u32>>)
        -> Poll<io::Result<&[u8]>>
    {
        if self.pos >= self.cap {
            if self.data == ptr::null_mut() { self.alloc() }
            let buf = unsafe {
                slice::from_raw_parts_mut(self.data, self.capacity as usize)
            };
            self.cap = ready!(fill(buf))?;
            self.pos = 0;
        }
        Poll::Ready(Ok(self.buffered_from_read()))
    }

    #[inline(always)]
    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt as u32, self.cap);
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.cap = 0;
    }

    pub fn cancellation(&mut self) -> Cancellation {
        self.clear();
        let data = mem::replace(&mut self.data, ptr::null_mut());
        unsafe { Cancellation::buffer(data, self.capacity as usize) }
    }

    fn alloc(&mut self) {
        let layout = Layout::array::<u8>(self.capacity as usize).unwrap();
        let ptr = unsafe { alloc(layout) };
        if ptr == ptr::null_mut() {
            handle_alloc_error(layout);
        }
        self.data = ptr;
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
