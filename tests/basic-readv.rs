use std::fs::File;
use std::os::unix::io::AsRawFd;

use ringbahn::drive::demo;
use ringbahn::event::ReadVectored;
use ringbahn::Submission;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn readv_file() {
    let file = File::open("props.txt").unwrap();
    let vec1: Box<[u8]> = Box::new([0; 4]);
    let vec2: Box<[u8]> = Box::new([0; 5]);
    let vec3: Box<[u8]> = Box::new([0; 10]);
    let readv = ReadVectored {
        fd: file.as_raw_fd(),
        bufs: vec![vec1, vec2, vec3].into_boxed_slice(),
        offset: 0,
    };
    let (readv, result) = futures::executor::block_on(Submission::new(readv, demo::driver()));
    assert!(result.is_ok());
    assert_eq!(readv.bufs[0][..], ASSERT[0..4]);
    assert_eq!(readv.bufs[1][..], ASSERT[4..9]);
    assert_eq!(readv.bufs[2][..], ASSERT[9..19]);
}
