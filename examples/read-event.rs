use ringbahn::*;
use std::fs::{metadata, File};
use std::io;
use std::os::unix::io::AsRawFd;

fn main() -> io::Result<()> {
    let driver = drive::demo::driver();
    let meta = metadata("props.txt")?;
    let file = File::open("props.txt")?;
    let event = event::Read {
        fd: file.as_raw_fd(),
        buf: vec![0; meta.len() as usize].into(),
        offset: 0
    };
    let submission = Submission::new(event, driver.clone());
    futures::executor::block_on(async move {
        let (event, result) = submission.await;
        let bytes_read = result? as usize;
        let content = String::from_utf8_lossy(&event.buf[0..bytes_read]).to_string();
        ringbahn::print!(driver, "{}", content).await;
        Ok(())
    })
}
