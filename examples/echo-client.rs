use ringbahn::net::TcpStream;

use std::io::{self, BufRead, Write};

use futures::executor::block_on;
use futures::io::{AsyncBufReadExt, AsyncWriteExt};

fn main() {
    block_on(async move {
        let mut stream = TcpStream::connect(("127.0.0.1", 7878)).await.unwrap();
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut stdout = stdout;
        let mut buf = String::new();

        for line in stdin.lock().lines() {
            let line = line.unwrap();
            stream.write_all(line.as_bytes()).await.unwrap();
            stream.read_line(&mut buf).await.unwrap();
            stdout.write_all(buf.as_bytes()).unwrap();
            buf.clear();
        }
    })
}
