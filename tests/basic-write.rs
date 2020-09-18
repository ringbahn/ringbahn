use std::io::Read;
use std::os::unix::io::AsRawFd;

use ringbahn::Submission;
use ringbahn::event::Write;
use ringbahn::drive::demo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn write_file() {
    let mut file = tempfile::tempfile().unwrap();
    let write = Write {
        fd: file.as_raw_fd(),
        buf: Box::from(ASSERT),
        offset: 0,
    };
    let (_, result) = futures::executor::block_on(Submission::new(write, demo::driver()));
    assert_eq!(result.unwrap() as usize, ASSERT.len());

    let mut buf = vec![];
    assert_eq!(file.read_to_end(&mut buf).unwrap(), ASSERT.len());
    assert_eq!(&buf[0..ASSERT.len()], ASSERT);
}
