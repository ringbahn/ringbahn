use std::fs::File;

use futures::AsyncReadExt;

use ringbahn::Ring;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn read_file() {
    let file = File::open("props.txt").unwrap();
    let mut file: Ring<File> = Ring::new(file);
    futures::executor::block_on(async move {
        let mut buf = vec![0; 4096];
        assert!(file.read(&mut buf).await.is_ok());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
    });
}

#[test]
fn read_to_end() {
    let file = File::open("props.txt").unwrap();
    let mut file: Ring<File> = Ring::new(file);
    futures::executor::block_on(async move {
        let mut buf = Vec::new();
        assert!(file.read_to_end(&mut buf).await.is_ok());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
    });
}
