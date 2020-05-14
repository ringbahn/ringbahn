use std::fs::File;

use futures::AsyncReadExt;

use ringbahn::Ring;
use ringbahn::driver::DRIVER;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn read_file() {
    let file = File::open("props.txt").unwrap();
    let mut file: Ring<File, _> = Ring::new(file, &DRIVER);
    futures::executor::block_on(async move {
        let mut buf = vec![0; 4096];
        assert!(file.read(&mut buf).await.is_ok());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
    });
}
