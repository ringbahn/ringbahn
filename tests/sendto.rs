use std::net::UdpSocket;
use std::os::unix::io::AsRawFd;
use nix::sys::socket::InetAddr;

use iou::sqe::{MsgFlags, SockAddr};

use ringbahn::Submission;
use ringbahn::drive::demo;
use ringbahn::event::SendTo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn sendto() -> std::io::Result<()> {
    let sender = UdpSocket::bind("127.0.0.1:3400").expect("couldn't bind to address");
    let receiver = UdpSocket::bind("127.0.0.1:3401").expect("couldn't bind to address");

    let buf: Box<[u8]> = Box::from(ASSERT);
    let recv_addr = SockAddr::Inet(InetAddr::from_std(&receiver.local_addr()?));

    let sendto = SendTo::new(sender.as_raw_fd(), buf, recv_addr, MsgFlags::empty());
    let (_, result) = futures::executor::block_on(Submission::new(sendto, demo::driver()));

    let res = result.unwrap();
    assert_eq!(res, ASSERT.len() as u32);

    let mut buf = [0; ASSERT.len()];
    let (amt, src) = receiver.recv_from(&mut buf)?;

    assert_eq!(amt, ASSERT.len());
    assert_eq!(src, sender.local_addr()?);
    assert_eq!(buf, ASSERT);
    Ok(())
}
