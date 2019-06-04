use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::{ReuseAddr, ReusePort};
use pnet_datalink as datalink;
use rand::{thread_rng, Rng};
use reqwest;
use socket2::{Domain, SockAddr, Socket, Type};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::os::unix::io::AsRawFd;

pub struct UdpSocketPair {
    pub addr: SocketAddr,    // Public address of the socket
    pub receiver: UdpSocket, // Locally bound socket that can receive from the public address
    pub sender: UdpSocket,   // Locally bound socket to send via public address
}

pub fn get_public_ip_addr() -> Result<IpAddr, String> {
    let body = reqwest::get("http://ifconfig.co/ip")
        .map_err(|err| err.to_string())?
        .text()
        .map_err(|err| err.to_string())?;

    match body.lines().next() {
        Some(ip) => Result::Ok(ip.parse().unwrap()),
        None => Result::Err("Empty response body".to_string()),
    }
}

pub fn parse_port_or_addr(optstr: Option<&str>, default_port: u16) -> SocketAddr {
    let daddr = SocketAddr::from(([0, 0, 0, 0], default_port));

    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = daddr;
            addr.set_port(port);
            addr
        } else if let Ok(addr) = addrstr.parse() {
            addr
        } else {
            daddr
        }
    } else {
        daddr
    }
}

fn find_eth0ish_ip_addr(ifaces: &mut Vec<datalink::NetworkInterface>) -> Option<IpAddr> {
    // put eth0 and wifi0, etc. up front of our list of candidates
    ifaces.sort_by(|a, b| {
        a.name
            .chars()
            .last()
            .unwrap()
            .cmp(&b.name.chars().last().unwrap())
    });

    for iface in ifaces.clone() {
        trace!("get_ip_addr considering iface {}", iface.name);
        for p in iface.ips {
            trace!("  ip {}", p);
            if p.ip().is_loopback() {
                trace!("    loopback");
                continue;
            }
            if p.ip().is_multicast() {
                trace!("    multicast");
                continue;
            }
            match p.ip() {
                IpAddr::V4(addr) => {
                    if addr.is_link_local() {
                        trace!("    link local");
                        continue;
                    }
                    trace!("    picked {}", p.ip());
                    return Some(p.ip());
                }
                IpAddr::V6(_addr) => {
                    // Select an ipv6 address if the config is selected
                    #[cfg(feature = "ipv6")]
                    {
                        return Some(p.ip());
                    }
                }
            }
        }
    }
    None
}

pub fn get_ip_addr() -> Option<IpAddr> {
    let mut ifaces = datalink::interfaces();

    find_eth0ish_ip_addr(&mut ifaces)
}

fn udp_socket(reuseaddr: bool) -> io::Result<Socket> {
    let sock = Socket::new(Domain::ipv4(), Type::dgram(), None)?;
    let sock_fd = sock.as_raw_fd();

    if reuseaddr {
        // best effort, i.e. ignore errors here, we'll get the failure in caller
        setsockopt(sock_fd, ReusePort, &true).ok();
        setsockopt(sock_fd, ReuseAddr, &true).ok();
    }

    Ok(sock)
}

pub fn bind_in_range(range: (u16, u16)) -> io::Result<(u16, UdpSocket)> {
    let sock = udp_socket(false)?;

    let (start, end) = range;
    let mut tries_left = end - start;
    loop {
        let rand_port = thread_rng().gen_range(start, end);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rand_port);

        match sock.bind(&SockAddr::from(addr)) {
            Ok(_) => {
                let sock = sock.into_udp_socket();
                break Result::Ok((sock.local_addr().unwrap().port(), sock));
            }
            Err(err) => if err.kind() != io::ErrorKind::AddrInUse || tries_left == 0 {
                return Err(err);
            },
        }
        tries_left -= 1;
    }
}

pub fn multi_bind_in_range(range: (u16, u16), num: usize) -> io::Result<(u16, Vec<UdpSocket>)> {
    let mut sockets = Vec::with_capacity(num);

    let port = {
        let (port, _) = bind_in_range(range)?;
        port
    }; // drop the probe, port should be available... briefly.

    for _ in 0..num {
        sockets.push(bind_to(port, true)?);
    }
    Ok((port, sockets))
}

pub fn bind_to(port: u16, reuseaddr: bool) -> io::Result<UdpSocket> {
    let sock = udp_socket(reuseaddr)?;

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);

    match sock.bind(&SockAddr::from(addr)) {
        Ok(_) => Result::Ok(sock.into_udp_socket()),
        Err(err) => Err(err),
    }
}

