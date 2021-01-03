use ringbahn::net::TcpListener;

use futures::executor::{block_on, ThreadPool};
use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::StreamExt;

fn main() {
    let mut listener = TcpListener::bind(("127.0.0.1", 7878)).unwrap();
    println!("listening on port 7878");
    let mut incoming = listener.incoming();
    let pool = ThreadPool::new().unwrap();
    block_on(async move {
        while let Some(stream) = incoming.next().await {
            println!("recieved connection");
            let (mut stream, _) = stream.unwrap();
            pool.spawn_ok(async move {
                loop {
                    let mut buf = [0; 8096];
                    let n = stream.read(&mut buf[..]).await.unwrap();
                    println!("read {} bytes", n);
                    buf[n] = b'\n';
                    stream.write_all(&buf[0..n + 1]).await.unwrap();
                    println!("write {} bytes", n + 1);
                }
            });
        }
    });
}
