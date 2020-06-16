use std::fs::File;

use ringbahn::Submission;
use ringbahn::event::Read;
use ringbahn::drive::demo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn read_file() {
    let file = File::open("props.txt").unwrap();
    let read: Read<'_, File> = Read::new(&file, vec![0; 4096], 0);
    let (read, result) = futures::executor::block_on(Submission::new(read, demo::driver()));
    assert!(result.is_ok());
    assert_eq!(&read.buf[0..ASSERT.len()], ASSERT);
}
