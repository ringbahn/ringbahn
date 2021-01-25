//! Networking primitives for TCP/UDP communication.
pub mod tcp;
pub mod udp;

pub use tcp::{Incoming, TcpListener, TcpStream};
#[doc(inline)]
pub use udp::UdpSocket;
