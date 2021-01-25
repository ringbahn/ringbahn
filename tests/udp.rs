use ringbahn::net::UdpSocket;

#[test]
fn udp_smoke_test() {
    let mut server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut client = UdpSocket::bind("127.0.0.1:0").unwrap();

    assert!(client.peer_addr().is_err());
    futures::executor::block_on(async {
        client.connect(server.local_addr().unwrap()).await.unwrap();
        server.connect(client.local_addr().unwrap()).await.unwrap();
    });

    assert_eq!(client.peer_addr().unwrap(), server.local_addr().unwrap());
    assert_eq!(server.peer_addr().unwrap(), client.local_addr().unwrap());

    let mut buf = [0; 10];
    server.inner.send(&[0,1,2]).expect("couldn't send message");

    // synchronous code works fine:
    //client.inner.recv(&mut buf).unwrap();
    //assert_eq!(&buf[0..3], &[0, 1, 2]);

    // panics with SIGILL
    futures::executor::block_on(async {
        client.recv(&mut buf).await.unwrap();
    });
    assert_eq!(&buf[0..2], &[0, 1, 2]);
}
