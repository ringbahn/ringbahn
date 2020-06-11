use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::io;
use std::mem::{MaybeUninit, ManuallyDrop};
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Poll, Context};
use std::slice;

use crate::Cancellation;
use super::Drive;

pub trait ProvideBuffer<D: Drive>: AsRef<[MaybeUninit<u8>]> + AsMut<[MaybeUninit<u8>]> + Sized {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>;
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);
    unsafe fn cleanup(this: &mut ManuallyDrop<Self>) -> Cancellation;
}

pub struct HeapBuffer {
    data: NonNull<MaybeUninit<u8>>,
    capacity: u32,
}

impl AsRef<[MaybeUninit<u8>]> for HeapBuffer {
    fn as_ref(&self) -> &[MaybeUninit<u8>] {
        unsafe {
            slice::from_raw_parts(self.data.as_ptr(), self.capacity as usize)
        }
    }
}

impl AsMut<[MaybeUninit<u8>]> for HeapBuffer {
    fn as_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe {
            slice::from_raw_parts_mut(self.data.as_ptr(), self.capacity as usize)
        }
    }
}

impl<D: Drive> ProvideBuffer<D> for HeapBuffer {
    fn poll_provide(_: Pin<&mut D>, _: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>
    {
        assert!(capacity <= u32::MAX as usize);
        unsafe {
            let layout = Layout::array::<MaybeUninit<u8>>(capacity).unwrap();
            let ptr = alloc(layout);
            if ptr.is_null() {
                handle_alloc_error(layout);
            }
            Poll::Ready(Ok(HeapBuffer {
                data: NonNull::new_unchecked(ptr).cast(),
                capacity: capacity as u32
            }))
        }
    }
    
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.raw_mut().addr = self.data.as_ptr() as usize as u64;
        sqe.raw_mut().len = self.capacity;
    }

    unsafe fn cleanup(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(this.data.cast().as_ptr(), this.capacity as usize)
    }
}

unsafe impl Send for HeapBuffer { }
unsafe impl Sync for HeapBuffer { }

impl Drop for HeapBuffer {
    fn drop(&mut self) {
        let layout = Layout::array::<MaybeUninit<u8>>(self.capacity as usize).unwrap();
        unsafe {
            dealloc(self.data.cast().as_ptr(), layout);
        }
    }
}
