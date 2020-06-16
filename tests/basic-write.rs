use std::fs::File;
use std::io::Read;

use ringbahn::Submission;
use ringbahn::event::Write;
use ringbahn::drive::demo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn write_file() {
    let mut file = tempfile::tempfile().unwrap();
    let write: Write<'_, File> = Write::new(&file, Vec::from(ASSERT), 0);
    let (_, result) = futures::executor::block_on(Submission::new(write, demo::driver()));
    assert_eq!(result.unwrap(), ASSERT.len());

    let mut buf = vec![];
    assert_eq!(file.read_to_end(&mut buf).unwrap(), ASSERT.len());
    assert_eq!(&buf[0..ASSERT.len()], ASSERT);
}
