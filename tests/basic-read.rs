use std::fs::File;
use std::os::unix::io::AsRawFd;

use ringbahn::Submission;
use ringbahn::event::Read;
use ringbahn::drive::demo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn read_file() {
    let file = File::open("props.txt").unwrap();
    let read = Read {
        fd: file.as_raw_fd(),
        buf: vec![0; 4096].into(),
        offset: 0,
    };
    let (read, result) = futures::executor::block_on(Submission::new(read, demo::driver()));
    assert!(result.is_ok());
    assert_eq!(&read.buf[0..ASSERT.len()], ASSERT);
}
