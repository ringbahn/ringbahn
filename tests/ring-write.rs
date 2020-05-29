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
