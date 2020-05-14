use std::fs::File;

use futures::io::AsyncReadExt;
use futures::io::AsyncWriteExt;

use ringbahn::Ring;

fn main() {
    let mut input:  Ring<File> = Ring::new(File::open("props.txt").unwrap());
    let mut output: Ring<File> = Ring::new(File::create("test.txt").unwrap());
    futures::executor::block_on(async move {
        let mut buf = vec![0; 1024];
        let len = input.read(&mut buf).await.unwrap();
        output.write(&mut buf[0..len]).await.unwrap();
        output.flush().await.unwrap();
    });
}
