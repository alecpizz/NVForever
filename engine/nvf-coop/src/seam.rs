//! The seam under the session: what carries the co-op packets, and who may join.
//!
//! A [`Transport`] sends a datagram to an address and hands back the next one waiting,
//! without blocking. The session ([`crate::net`]) knows its peers by [`SocketAddr`] and
//! nothing else of how the bytes travel, so a real transport (an encrypted one, one that
//! crosses routers) plugs in under it unchanged. No real transport is in this part: the
//! tests carry packets in memory ([`memory`]) or over loopback UDP ([`loopback`]).
//!
//! Who may join is the other half of the seam: an [`Admission`]. [`SharedSecret`] is the
//! interim one the tests use; the real transport ([`crate::stack::Stack`], Noise) proves
//! each peer's key, and its admission ([`crate::stack::KeyAdmission`]) takes the key the
//! sender's address proved, not anything the Hello says.

use std::net::SocketAddr;

/// A datagram transport, keyed by address. Datagrams may be lost, doubled or reordered;
/// the session copes with all three.
pub trait Transport: Send {
    /// Send one datagram to `to`; the bytes sent.
    fn send_to(&self, bytes: &[u8], to: SocketAddr) -> std::io::Result<usize>;
    /// Non-blocking: Ok(None) when nothing is waiting.
    fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<Option<(usize, SocketAddr)>>;
    fn local_addr(&self) -> std::io::Result<SocketAddr>;
}

/// Who may join: a guest's Hello carries a proof ([`Admission::hello`]) the host checks
/// before anything else, a returning guest's token included ([`Admission::admits`]), and
/// the host's answer (a Welcome, or Full) carries a proof back
/// ([`Admission::answer`]) that the guest checks before it takes the host as its host
/// ([`Admission::answered`]). [`SharedSecret`] is the interim one: a secret the host's
/// lobby opens with and its guests are given. An encrypted transport that authenticates
/// its peers replaces it.
pub trait Admission: Send {
    /// What a guest's Hello carries.
    fn hello(&self) -> u64;
    /// Host: whether a Hello carrying `proof` may be let in (or known again by its token).
    fn admits(&self, proof: u64) -> bool;
    /// Host: the proof its answer to the guest with `token` carries: a Welcome as `id`, or
    /// Full (None).
    fn answer(&self, token: u64, id: Option<u8>) -> u64;
    /// Guest: whether an answer's proof came from a host that holds the lobby's secret.
    fn answered(&self, token: u64, id: Option<u8>, proof: u64) -> bool {
        self.answer(token, id) == proof
    }
    /// Host: whether the Hello from `from` carrying `proof` and `token` may be let in. An
    /// admission that knows who is at an address (an authenticating transport) decides by
    /// that; the default is [`Admission::admits`].
    fn admits_from(&self, proof: u64, token: u64, from: SocketAddr) -> Admit {
        let _ = (token, from);
        if self.admits(proof) {
            Admit::Yes
        } else {
            Admit::No
        }
    }
    /// Host: the player at `at` is banned while the lobby lasts (Kick with Ban); false when
    /// this admission cannot tell players apart (a shared secret).
    fn ban(&mut self, at: SocketAddr) -> bool {
        let _ = at;
        false
    }
}

/// What the host does with a Hello ([`Admission::admits_from`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Yes,
    /// Dropped without a word (a stranger, a forged token).
    No,
    /// Told it is banned ([`crate::net::Msg::Kicked`]), so its game stops asking.
    Banned,
}

/// The lobby's secret, shared by its host and guests, never 0 (module notes and
/// [`Admission`]). A guest's Hello carries it as it is; the host's answer carries a value
/// keyed by it, the guest's token and its id, so an answer cannot be made without the
/// secret. Not cryptography: an interim check against strangers and stray hosts until an
/// authenticated transport takes over.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SharedSecret(u64);

impl std::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("SharedSecret(..)")
    }
}

impl SharedSecret {
    /// A new lobby's secret, from the OS random source.
    pub fn random() -> std::io::Result<Self> {
        loop {
            let v = getrandom::u64().map_err(|e| std::io::Error::other(e.to_string()))?;
            if let Some(s) = Self::new(v) {
                return Ok(s);
            }
        }
    }

    /// A secret as the host gave it; None for 0, which admits nobody.
    pub fn new(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

/// A 64-bit mixer (the splitmix64 finaliser): every bit of the input moves about half of
/// the output's.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Admission for SharedSecret {
    fn hello(&self) -> u64 {
        self.0
    }

    fn admits(&self, proof: u64) -> bool {
        proof != 0 && proof == self.0
    }

    fn answer(&self, token: u64, id: Option<u8>) -> u64 {
        let id = id.map_or(0x100, u64::from);
        mix(mix(self.0 ^ mix(token)) ^ id).max(1)
    }
}

/// An in-memory transport for tests: endpoints on one [`memory::Hub`] reach each other
/// by address through channels, with no socket and nothing lost.
#[cfg(test)]
pub(crate) mod loopback {
    use super::Transport;
    use std::io::ErrorKind;
    use std::net::{SocketAddr, UdpSocket};

    /// A non-blocking UDP socket on this PC's loopback, for tests only.
    pub(crate) struct Udp(UdpSocket);

    impl Udp {
        /// A socket on `127.0.0.1:port` (0 picks a free one).
        pub(crate) fn host(port: u16) -> std::io::Result<Self> {
            let sock = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], port)))?;
            sock.set_nonblocking(true)?;
            Ok(Self(sock))
        }

        /// A socket on a free loopback port.
        pub(crate) fn guest() -> std::io::Result<Self> {
            Self::host(0)
        }
    }

    impl Transport for Udp {
        fn send_to(&self, bytes: &[u8], to: SocketAddr) -> std::io::Result<usize> {
            self.0.send_to(bytes, to)
        }

        fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<Option<(usize, SocketAddr)>> {
            loop {
                match self.0.recv_from(buf) {
                    Ok(got) => return Ok(Some(got)),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
                    // Windows reports an ICMP "port unreachable" from an earlier send as an
                    // error on a later receive; it says nothing about any packet waiting.
                    Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        fn local_addr(&self) -> std::io::Result<SocketAddr> {
            self.0.local_addr()
        }
    }
}

#[cfg(test)]
pub(crate) mod memory {
    use super::Transport;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};

    type Inbox = Sender<(Vec<u8>, SocketAddr)>;

    /// The endpoints, by address.
    #[derive(Clone, Default)]
    pub(crate) struct Hub(Arc<Mutex<HashMap<SocketAddr, Inbox>>>);

    impl Hub {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// An endpoint at `addr` (one already there is replaced: a game started again).
        pub(crate) fn endpoint(&self, addr: SocketAddr) -> Memory {
            let (tx, rx) = channel();
            self.0.lock().unwrap().insert(addr, tx);
            Memory { addr, hub: self.clone(), rx }
        }
    }

    /// Two endpoints that reach each other.
    pub(crate) fn pair(a: SocketAddr, b: SocketAddr) -> (Memory, Memory) {
        let hub = Hub::new();
        (hub.endpoint(a), hub.endpoint(b))
    }

    pub(crate) struct Memory {
        addr: SocketAddr,
        hub: Hub,
        rx: Receiver<(Vec<u8>, SocketAddr)>,
    }

    impl Transport for Memory {
        fn send_to(&self, bytes: &[u8], to: SocketAddr) -> std::io::Result<usize> {
            // To an address nobody holds: lost, as UDP's would be.
            if let Some(tx) = self.hub.0.lock().unwrap().get(&to) {
                let _ = tx.send((bytes.to_vec(), self.addr));
            }
            Ok(bytes.len())
        }

        fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<Option<(usize, SocketAddr)>> {
            match self.rx.try_recv() {
                Ok((bytes, from)) => {
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    Ok(Some((n, from)))
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
            }
        }

        fn local_addr(&self) -> std::io::Result<SocketAddr> {
            Ok(self.addr)
        }
    }

    #[test]
    fn a_pair_reaches_each_other_by_address() {
        let a: SocketAddr = "10.0.0.1:1".parse().unwrap();
        let b: SocketAddr = "10.0.0.2:2".parse().unwrap();
        let (x, mut y) = pair(a, b);
        let mut buf = [0u8; 16];
        assert_eq!(y.recv_from(&mut buf).unwrap(), None);
        x.send_to(&[1, 2, 3], b).unwrap();
        x.send_to(&[9], "10.0.0.9:9".parse().unwrap()).unwrap();
        assert_eq!(y.recv_from(&mut buf).unwrap(), Some((3, a)));
        assert_eq!(&buf[..3], &[1, 2, 3]);
        assert_eq!(y.recv_from(&mut buf).unwrap(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::loopback::Udp;
    use super::*;

    #[test]
    fn a_shared_secret_admits_only_itself_and_proves_it_back() {
        let s = SharedSecret::new(0x1234_5678_9ABC_DEF0).unwrap();
        let other = SharedSecret::new(0x1234_5678_9ABC_DEF1).unwrap();
        assert!(SharedSecret::new(0).is_none());
        assert!(s.admits(s.hello()));
        assert!(!s.admits(other.hello()));
        assert!(!s.admits(0));
        // An answer is keyed by the secret, the token and the id.
        assert!(s.answered(7, Some(2), s.answer(7, Some(2))));
        assert!(!s.answered(7, Some(2), other.answer(7, Some(2))));
        assert!(!s.answered(7, Some(2), s.answer(8, Some(2))));
        assert!(!s.answered(7, Some(2), s.answer(7, Some(3))));
        assert!(!s.answered(7, None, s.answer(7, Some(0))));
        let r = SharedSecret::random().unwrap();
        assert_ne!(r.value(), 0);
        assert_ne!(r, SharedSecret::random().unwrap());
        assert_eq!(format!("{s:?}"), "SharedSecret(..)");
    }

    #[test]
    fn udp_sockets_talk_and_never_block() {
        let mut host = Udp::host(0).unwrap();
        let guest = Udp::guest().unwrap();
        let mut buf = [0u8; 64];
        assert_eq!(host.recv_from(&mut buf).unwrap(), None);
        let to = SocketAddr::from(([127, 0, 0, 1], host.local_addr().unwrap().port()));
        guest.send_to(b"hi", to).unwrap();
        let mut got = None;
        for _ in 0..200 {
            got = host.recv_from(&mut buf).unwrap();
            if got.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let (n, _) = got.expect("the datagram arrives");
        assert_eq!(&buf[..n], b"hi");
    }
}
