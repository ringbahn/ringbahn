use std::fs::File;
use std::io::Read;

use futures::AsyncWriteExt;

use ringbahn::Ring;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn write_file() {
    let file = tempfile::tempfile().unwrap();
    let mut file: Ring<File> = Ring::new(file);
    futures::executor::block_on(async move {
        assert_eq!(file.write(ASSERT).await.unwrap(), ASSERT.len());

        let mut buf = vec![];
        assert_eq!(file.blocking().read_to_end(&mut buf).unwrap(), ASSERT.len());
        assert_eq!(&buf[0..ASSERT.len()], ASSERT);
    });
}

#[test]
fn select_complete_many_futures() {
    async fn act() {
        let file = tempfile::tempfile().unwrap();
        let mut file: Ring<File> = Ring::new(file);
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
