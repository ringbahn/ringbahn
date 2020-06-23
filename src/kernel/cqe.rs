use std::io;

pub struct CQE {
    user_data: u64,
    res: i32,
    flags: u32,
}

impl CQE {
    pub(crate) fn from_raw(cqe: uring_sys::io_uring_cqe) -> CQE {
        CQE { user_data: cqe.user_data, res: cqe.res, flags: cqe.flags }
    }

    pub fn complete(self) {
        crate::completion::complete(self)
    }

    pub fn result(&self) -> io::Result<u32> {
        match self.res {
            n if n >= 0     => Ok(n as u32),
            err             => Err(io::Error::from_raw_os_error(err)),
        }
    }

    pub fn flags(&self) -> u32 {
        self.flags
    }

    pub fn user_data(&self) -> u64 {
        self.user_data
    }
}
