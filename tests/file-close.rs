use futures::{AsyncReadExt, AsyncWriteExt};

use ringbahn::fs::File;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn read_and_close_file() {
    futures::executor::block_on(async move {
        let mut file = File::open("props.txt").await.unwrap();
        let mut buf = vec![0; 4096];
        assert!(file.read(&mut buf).await.is_ok());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
        file.close().await.unwrap();
    });
}
