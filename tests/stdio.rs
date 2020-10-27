use futures::AsyncWriteExt;

use ringbahn::{drive::demo, io::Stdout};

const ASSERT: &[u8] = b"Hello, world!\n";

#[test]
fn write_stdout() {
    futures::executor::block_on(async {
        let n = ringbahn::io::stdout().write(ASSERT).await.unwrap();
        assert_eq!(n, ASSERT.len());
        let mut stdout = Stdout::run_on_driver(demo::driver());
        let n = stdout.write(ASSERT).await.unwrap();
        assert_eq!(n, ASSERT.len());
    });
}
