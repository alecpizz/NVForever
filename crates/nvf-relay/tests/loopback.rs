//! The relay on real sockets: two clients on loopback bind their sides of a session and
//! exchange datagrams through it.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use nvf_nat::hosted::{encode, session_id, Client, Op, Received, Side};
use nvf_relay::{Relay, ServerConfig};

fn recv(sock: &UdpSocket) -> Vec<u8> {
    let mut buf = [0u8; 2048];
    let (n, _) = sock.recv_from(&mut buf).expect("an answer from the relay");
    buf[..n].to_vec()
}

#[test]
fn forwards_between_two_clients_on_loopback() {
    let relay = Relay::start(&["127.0.0.1:0".parse().unwrap()], ServerConfig::default()).unwrap();
    let at = relay.addrs[0];
    let session = session_id(&[1; 32], &[2; 32]);
    let socks: Vec<UdpSocket> = (0..2).map(|_| UdpSocket::bind("127.0.0.1:0").unwrap()).collect();
    socks.iter().for_each(|s| s.set_read_timeout(Some(Duration::from_secs(2))).unwrap());
    let mut clients = [Client::new(at, session, Side::Host), Client::new(at, session, Side::Guest)];
    let now = Instant::now();
    for (c, s) in clients.iter_mut().zip(&socks) {
        // First Bind from an unproven address gets a Challenge; echoing its cookie yields Bound.
        s.send_to(&c.poll(now).unwrap(), at).unwrap();
        assert_eq!(c.on_datagram(&recv(s)), Received::Control);
        assert!(!c.bound(), "first reply is a return-check challenge, not Bound");
        s.send_to(&c.poll(now).unwrap(), at).unwrap();
        assert_eq!(c.on_datagram(&recv(s)), Received::Control);
        assert!(c.bound());
    }
    // Data from an unproven third socket is dropped and never forwarded.
    let rogue = UdpSocket::bind("127.0.0.1:0").unwrap();
    rogue.send_to(&encode(Op::Data, &session, Side::Guest, b"spoofed"), at).unwrap();

    socks[1].send_to(&clients[1].wrap(b"ciphertext from the guest"), at).unwrap();
    let got = recv(&socks[0]);
    assert_eq!(clients[0].on_datagram(&got), Received::Data(b"ciphertext from the guest"));
    socks[0].send_to(&clients[0].wrap(b"and back"), at).unwrap();
    let got = recv(&socks[1]);
    assert_eq!(clients[1].on_datagram(&got), Received::Data(b"and back"));
    assert_eq!(relay.stats().forwarded, 2);
    assert_eq!(relay.sessions(), 1);
    relay.stop();
}
