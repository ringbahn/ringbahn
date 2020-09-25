use std::any::{Any, TypeId};
use std::mem::ManuallyDrop;

use crate::Cancellation;

#[derive(Default)]
pub struct Data {
    inner: Option<Inner>
}

impl Data {
    pub fn alloc_bytes(&mut self, len: usize) -> &mut [u8] {
        if self.inner.is_none() {
            self.inner = Some(Inner::Buffer(vec![0; len].into_boxed_slice()));
        }
        self.inner.as_mut().unwrap().bytes_mut().unwrap()
    }

    pub fn alloc<T: UringData>(&mut self, data: T) -> &mut T  {
        if self.inner.is_none() {
            self.inner = Some(Inner::Object(Box::new(data)));
        }
        self.inner.as_mut().unwrap().downcast().unwrap()
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        self.inner.as_ref().and_then(|inner| inner.bytes())
    }

    pub fn cancellation(&mut self) -> Cancellation {
        self.inner.take().map_or_else(Cancellation::null, Inner::cancellation)
    }
}

enum Inner {
    Buffer(Box<[u8]>),
    Object(Box<dyn UringData>),
}

impl Inner {
    fn downcast<T: UringData>(&mut self) -> Option<&mut T> {
        match self {
            Inner::Object(object)   => object.downcast(),
            Inner::Buffer(_)        => None,
        }
    }

    fn bytes_mut(&mut self) -> Option<&mut [u8]> {
        match self {
            Inner::Buffer(bytes)    => Some(&mut bytes[..]),
            Inner::Object(_)        => None,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Inner::Buffer(bytes)    => Some(&bytes[..]),
            Inner::Object(_)        => None,
        }
    }

    fn cancellation(self) -> Cancellation {
        match self {
            Inner::Buffer(bytes)    => {
                let mut bytes = ManuallyDrop::new(bytes);
                unsafe { Cancellation::buffer(bytes.as_mut_ptr(), bytes.len()) }
            }
            Inner::Object(object)   => {
                let mut object = ManuallyDrop::new(object);
                unsafe { object.cancellation() }
            }
        }
    }
}

pub trait UringData: Send + Sync + Any {
    /// # Safety
    ///
    /// This method must logically take ownership of `self`.
    unsafe fn cancellation(&mut self) -> Cancellation;
}

impl dyn UringData + 'static {
    fn downcast<T: UringData>(&mut self) -> Option<&mut T> {
        if Any::type_id(self) == TypeId::of::<T>() {
            unsafe { Some(&mut *(self as *mut dyn UringData as *mut T)) }
        } else {
            None
        }
    }
}

impl UringData for libc::statx {
    unsafe fn cancellation(&mut self) -> Cancellation {
        unsafe fn callback(ptr: *mut (), _: usize) {
            drop(Box::from_raw(ptr as *mut libc::statx));
        }
        Cancellation::new(self as *mut libc::statx as *mut (), 0, callback)
    }
}
