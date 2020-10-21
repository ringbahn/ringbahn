use std::io::{IoSlice, SeekFrom};

use futures::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use ringbahn::fs::File;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn write_file() {
    let file = tempfile::tempfile().unwrap();
    let mut file = File::from(file);
    futures::executor::block_on(async move {
        assert_eq!(file.write(ASSERT).await.unwrap(), ASSERT.len());

        let mut buf = vec![];
        assert!(file.seek(SeekFrom::Start(0)).await.is_ok());
        assert_eq!(file.read_to_end(&mut buf).await.unwrap(), ASSERT.len());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
    });
}

#[test]
fn writev_file() {
    let file = tempfile::tempfile().unwrap();
    let mut file = File::from(file);
    let bufs = &[
        IoSlice::new(&ASSERT[0..4]),
        IoSlice::new(&ASSERT[4..9]),
        IoSlice::new(&ASSERT[9..]),
    ];

    futures::executor::block_on(async move {
        assert_eq!(file.write_vectored(bufs).await.unwrap(), ASSERT.len());
        let mut buf = vec![];
        assert!(file.seek(SeekFrom::Start(0)).await.is_ok());
        assert_eq!(file.read_to_end(&mut buf).await.unwrap(), ASSERT.len());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
    });
}

#[test]
fn select_complete_many_futures() {
    async fn act() {
        let file = tempfile::tempfile().unwrap();
        let mut file = File::from(file);
        file.write_all(b"hello, world!").await.unwrap();
    }

    futures::executor::block_on(async move {
        use futures::FutureExt;

        let mut f1 = Box::pin(act().fuse());
        let mut f2 = Box::pin(act().fuse());
        let mut f3 = Box::pin(act().fuse());
        let mut f4 = Box::pin(act().fuse());
        let mut f5 = Box::pin(act().fuse());
        let mut f6 = Box::pin(act().fuse());
        let mut f7 = Box::pin(act().fuse());
        let mut f8 = Box::pin(act().fuse());
        loop {
            futures::select! {
                _ = f1  => (),
                _ = f2  => (),
                _ = f3  => (),
                _ = f4  => (),
                _ = f5  => (),
                _ = f6  => (),
                _ = f7  => (),
                _ = f8  => (),
                complete => break,
            }
        }
    });
}
