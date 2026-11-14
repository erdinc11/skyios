use crate::util::die;
use socket2::{Domain, Socket, Type};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

pub fn main(args: &[String]) -> i32 {
    let Some(port) = args.first().and_then(|p| p.parse::<u16>().ok()) else {
        eprintln!("usage: sweet udp-listen PORT");
        return 2;
    };
    let s = Socket::new(Domain::IPV4, Type::DGRAM, None).unwrap_or_else(|e| die(e));
    s.set_reuse_address(true).unwrap_or_else(|e| die(e));
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into()).unwrap_or_else(|e| die(format!("bind {port}: {e}")));
    let s: UdpSocket = s.into();
    let mut buf = vec![0u8; 65536];
    let mut out = std::io::stdout().lock();
    loop {
        let n = s.recv(&mut buf).unwrap_or_else(|e| die(e));
        if out.write_all(&buf[..n]).and_then(|_| out.flush()).is_err() {
            return 0;
        }
    }
}
