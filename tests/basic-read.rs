use std::fs::File;

use ringbahn::event::{Read, Event};
use ringbahn::DRIVER;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn read_file() {
    let mut file = File::open("props.txt").unwrap();
    let read: Read<'_, File> = Read::new(&mut file, vec![0; 4096]);
    let (read, result) = async_std::task::block_on(async move { read.submit(&DRIVER).await });
    assert!(result.is_ok());
    assert_eq!(&read.buf[0..ASSERT.len()], ASSERT);
}
