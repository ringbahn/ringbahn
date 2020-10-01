use futures::io::AsyncReadExt;
use futures::io::AsyncWriteExt;

use ringbahn::fs::File;

fn main() {
    futures::executor::block_on(async move {
        let mut input:  File = File::open("props.txt").await.unwrap();
        let mut output: File = File::create("test.txt").await.unwrap();
        let mut buf = vec![0; 1024];
        let len = input.read(&mut buf).await.unwrap();
        output.write(&mut buf[0..len]).await.unwrap();
        output.flush().await.unwrap();
    });
}
