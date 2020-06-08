use std::io::SeekFrom;
use futures::{AsyncSeekExt, AsyncReadExt, AsyncWriteExt};

use ringbahn::File;

#[test]
fn seek_to_end() {
    futures::executor::block_on(async move {
        let mut file = File::open("props.txt").await.unwrap();
        assert_eq!(file.seek(SeekFrom::End(0)).await.unwrap(), 792);
    });
}

#[test]
fn seek_and_then_io() {
    futures::executor::block_on(async move {
        let mut file: File = tempfile::tempfile().unwrap().into();
        assert_eq!(file.seek(SeekFrom::End(0)).await.unwrap(), 0);
        file.write(b"abcdef").await.unwrap();
        let mut buf = [0; 16];
        assert_eq!(file.seek(SeekFrom::Start(0)).await.unwrap(), 0);
        file.read(&mut buf).await.unwrap();
        assert_eq!(&buf[0..6], b"abcdef");
    });
}
