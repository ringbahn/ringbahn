mod data;

use std::any::Any;
use std::cmp;
use std::io;
use std::task::Poll;

use futures_core::ready;

use crate::Cancellation;

use data::Data;

pub struct Buffer {
    data: Data,
    pos: u32,
    cap: u32,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            data: Data::default(),
            pos: 0,
            cap: 0,
        }
    }

    pub fn buffered_from_read(&self) -> &[u8] {
        self.data.as_bytes().map_or(&[], |data| &data[self.pos as usize..self.cap as usize])
    }

    pub fn fill_buf(&mut self, fill: impl FnOnce(&mut [u8]) -> Poll<io::Result<u32>>)
        -> Poll<io::Result<&[u8]>>
    {
        if self.pos >= self.cap {
            self.cap = ready!(fill(self.data.alloc_bytes(4096 * 2)))?;
            self.pos = 0;
        }
        Poll::Ready(Ok(self.buffered_from_read()))
    }

    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt as u32, self.cap);
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.cap = 0;
    }

    pub fn as_object<T: Any + Send + Sync>(&mut self, callback: impl FnOnce() -> T) -> &mut T {
        self.data.alloc(callback)
    }

    pub fn cancellation(&mut self) -> Cancellation {
        self.data.cancellation()
    }
}
