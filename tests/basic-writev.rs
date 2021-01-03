use std::io::Read;
use std::os::unix::io::AsRawFd;

use ringbahn::drive::demo;
use ringbahn::event::WriteVectored;
use ringbahn::Submission;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn writev_file() {
    let mut file = tempfile::tempfile().unwrap();
    let vec1 = &ASSERT[0..4];
    let vec2 = &ASSERT[4..9];
    let vec3 = &ASSERT[9..];
    let writev = WriteVectored {
        fd: file.as_raw_fd(),
        bufs: vec![vec1.into(), vec2.into(), vec3.into()].into(),
        offset: 0,
    };
    let (_, result) = futures::executor::block_on(Submission::new(writev, demo::driver()));
    assert_eq!(result.unwrap() as usize, ASSERT.len());

    let mut buf = vec![];
    assert_eq!(file.read_to_end(&mut buf).unwrap(), ASSERT.len());
    assert_eq!(&buf[0..ASSERT.len()], ASSERT);
}
