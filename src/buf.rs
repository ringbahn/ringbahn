use std::cmp;
use std::io;
use std::task::Poll;

use futures_core::ready;

use crate::ring::Cancellation;

#[derive(Default, Debug)]
pub struct Buffer {
    data: Option<Box<[u8]>>,
    pos: u32,
    cap: u32,
}

impl Buffer {
    pub fn buffered_from_read(&self) -> &[u8] {
        self.as_bytes().map_or(&[], |data| &data[self.pos as usize..self.cap as usize])
    }

    pub fn fill_buf(&mut self, fill: impl FnOnce(&mut [u8]) -> Poll<io::Result<u32>>)
        -> Poll<io::Result<&[u8]>>
    {
        if self.pos >= self.cap {
            self.cap = ready!(fill(self.alloc_bytes(4096 * 2)))?;
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

    pub fn into_boxed_slice(self) -> Option<Box<[u8]>> {
        self.data
    }

    pub fn cancellation(&mut self) -> Cancellation {
        Cancellation::from(self.data.take())
    }

    fn as_bytes(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }

    fn alloc_bytes(&mut self, cap: usize) -> &mut [u8] {
        if self.data.is_none() {
            self.data = Some(vec![0; cap].into_boxed_slice());
        }

        self.data.as_deref_mut().unwrap()
    }
}
