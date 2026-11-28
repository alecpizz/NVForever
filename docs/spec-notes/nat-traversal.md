# NAT traversal: STUN, PCP, NAT-PMP and the checks

Status: 2026-11-24. The formats `crates/nvf-nat` implements, for the co-op networking design
(#3795 sections 7.1 and 7.2). All three protocols are public IETF standards; nothing here comes
from the game. Tags as the charter asks.

## STUN Binding (RFC 8489)

- A message is a 20-byte header and attributes [documented: RFC 8489 section 5]:
  - Type (2 bytes): Binding request 0x0001, success response 0x0101. The top two bits are 0.
  - Length (2 bytes): bytes after the header, a multiple of 4.
  - Magic cookie 0x2112A442 (4 bytes).
  - Transaction id (12 bytes), random, copied into the answer.
- An attribute is type (2), length of the value before padding (2), the value, then zero
  padding to a multiple of 4 [documented: section 14].
- Attribute types read or written [documented: section 18.3]: MAPPED-ADDRESS 0x0001, ERROR-CODE
  0x0009, XOR-MAPPED-ADDRESS 0x0020, SOFTWARE 0x8022, FINGERPRINT 0x8028.
- XOR-MAPPED-ADDRESS [documented: section 14.2]:
  - A zero byte, the family (1 for IPv4, 2 for IPv6), the port, then the address.
  - The port is XORed with 0x2112.
  - An IPv4 address is XORed with the cookie. An IPv6 address is XORed with the cookie followed
    by the transaction id.
  - RFC 5769 sections 2.2 and 2.3 give worked values, used as the unit tests [documented].
- FINGERPRINT is CRC-32 (gzip's) over everything before it, XOR 0x5354554E. The header length
  already counts the FINGERPRINT attribute, and the attribute comes last [documented: section 14.7].
- Resends: 500 ms, doubling, the same transaction id, 7 sends at most [documented: section
  6.2.1]. NVForever stops at 3 s [design].
- Two servers that see the same public address mean one public port for every destination.
  Different ports mean a new port per destination (a "symmetric" NAT) [design, the method of
  RFC 4787's terms].

## PCP MAP (RFC 6887)

- Requests and answers go to the gateway's UDP port 5351 [documented: section 19.1].
- Request: version 2, opcode 1 (MAP), 2 reserved bytes, lifetime (4), the client's own
  address as 16 bytes, then the MAP data [documented: sections 7.1 and 11.1]:
  - nonce (12, random);
  - protocol (1, 17 for UDP) and 3 reserved bytes;
  - internal port (2);
  - suggested external port (2) and address (16).
- IPv4 addresses are written as IPv4-mapped IPv6 (`::ffff:a.b.c.d`) [documented: section 5].
- Answer: the same layout. Byte 1 has its top bit set, byte 3 is the result code, bytes 4 to
  7 the lifetime granted, 8 to 11 the epoch. Bytes 42 to 59 hold the external port and address
  given [documented: sections 7.2 and 11.1].
- The client checks the nonce. It ignores answers under 24 bytes, over 1100 bytes or not a
  multiple of 4 [documented: section 8.3].
- Result 1 means an unsupported version [documented: section 7.4].
- Renew from half the lifetime [documented: section 11.2.1]. Lifetime 0 removes the mapping.
- RFC 6887 asks for 3 s before the first resend [documented: section 8.1.1]. NVForever resends
  at 250 ms, doubling, and gives up at 2 s, so a router without PCP does not hold a join up
  [design].

## NAT-PMP (RFC 6886)

- Version 0, on the same port 5351. An answer's opcode is 128 plus the request's
  [documented: sections 3 and 3.1].
- External address request: `00 00`. Answer (12 bytes): version, 128, result (2), epoch (4),
  IPv4 address (4) [documented: section 3.2].
- UDP map request (12 bytes): version, opcode 1, 2 reserved bytes, internal port (2), suggested
  external port (2), lifetime (4). Answer (16 bytes): version, 129, result (2), epoch (4),
  internal port (2), external port (2), lifetime (4) [documented: section 3.3]. Lifetime 0
  removes the mapping [documented: section 3.4].
- An unsupported request gets a version-0 answer with result 1 [documented: section 3.5]. That
  is how a client that tried PCP first finds a NAT-PMP-only router.
- Only answers from the gateway count [documented: section 3.2].

## The checks (ICE, cut down)

[design: #3795 section 7.2; the triggered-check and peer-reflexive rules follow RFC 8445's
ideas, not its message format]

- Every local socket is paired with every remote candidate of its family.
- Pairs rank IPv6 direct, then LAN, then a mapped port or a public host address, then a STUN-seen
  or peer-learnt address.
- Both sides send one probe every 20 ms, round-robin over the unanswered pairs, for 10 s.
  The probes are Noise handshake messages, sent by the caller.
- A probe that arrives puts its pair at the front of the queue (a triggered check).
- An answer from an address that is not the one probed teaches a new, peer-reflexive
  candidate. That is how a symmetric NAT's per-destination port is found.
- The guest chooses the pair. It picks the best class that answered, at once if no better class
  is left to try, otherwise after 200 ms. Then it tells the host over that pair.
- On the chosen pair, a keepalive goes every 1 s while nothing else is sent. After 4 s without
  hearing from the other side, the path counts as lost.

## Simulated NATs

`natsim` models six kinds by mapping and filtering: open, full cone, restricted,
port-restricted, symmetric and blocked. It does no hairpinning. Its matrix test expects every
pair to connect except these [design: the textbook result]:

- anything with a blocked side;
- symmetric against symmetric;
- symmetric against port-restricted.

Two machines on one LAN connect over the LAN behind any kind.

## Port prediction (off by default)

[design: #3795 section 7.2's phase 5; the idea is the common one of guessing a symmetric NAT's
next port, our own rule for when and how far]

- A symmetric NAT gives every new destination a new public port, often the next free one.
- When a side's two STUN servers saw two different ports on one address, it is such a NAT.
  The other side then also probes the N ports above the highest one seen (`Config::predict`;
  0 turns it off and is the default, because it looks like a port scan to some firewalls).
- In `natsim`, whose symmetric NAT hands out ports in order, N = 8 connects symmetric
  against port-restricted (both ways round). Symmetric against symmetric still fails: each
  probe to a new guessed port opens yet another mapping on the prober's own NAT. That case is
  left to the peer relay and `nvf-relay`.
- A carrier NAT with random ports defeats this guess. When the other side is easy, the
  birthday punch below covers that case.

## Birthday punch (an easy NAT against a hard one)

[design: #4263. The idea is the "birthday paradox" punch Tailscale describes [documented:
Tailscale blog, "How NAT traversal works", 2020, its section on hard NATs]. The numbers, the
timing and the messages are ours.]

The case it is for [observed: two-machine test 2026-11-25, #4263]:
- The host was on home broadband: one public port for every server (endpoint-independent
  mapping), but it only let in packets from an address and port it had sent to.
- The guest was on a phone's mobile data: a carrier NAT with a new random port for each
  destination.
- Each side's probes reached the other's NAT from a port that NAT had never sent to, and
  were dropped. Both ends logged that no direct path was found.

How it works:
- **Who does what:** each side decides alone, from what it already has.
  - The *spread* side: its own STUN gather saw a hard NAT (`report::Nat::Hard`), and the
    other side has exactly one server-reflexive port per address.
  - The *spray* side: its own gather saw an easy NAT, and the other side has two or more
    server-reflexive ports on one address.
  - Any other pairing does nothing new.
- **When:** from 1 s after the checks start (`Config::birthday_after`), while no pair other
  than a hosted relay has answered. A LAN or an easier path wins long before.
- **Spread side:**
  - The agent asks the caller for 256 extra UDP sockets (`Action::OpenSockets`,
    `Config::birthday_sockets`; 0 turns it all off).
  - Once the caller hands their addresses back (`Agent::add_extra_sockets`), each extra
    socket sends one probe to the other side's server-reflexive address. That leaves 256
    mappings on its NAT, each letting in packets from that one address.
  - The caller listens on the extra sockets too, and answers each probe from the socket it
    came in on.
- **Spray side:**
  - From 300 ms after that, so the spread side's mappings exist first, it probes the other
    side's public IP from its own socket: one port every 4 ms (`Config::spray_pace`).
  - The ports are a fixed sweep over 1024–65535 in steps of 4099. The step is prime, so the
    sweep visits every port once before repeating: no repeats, and no random source needed.
  - Most probes are dropped. One that lands on a mapped port gets in, because it comes from
    the very address that socket sent to.
  - The answer from that socket then passes the spray side's own NAT, which has just sent to
    that port. The pair becomes valid as a peer-reflexive one, as for any symmetric NAT.
- **Afterwards:** on the choice, the caller closes the extra sockets except the chosen one
  (`Action::CloseSockets`), or all of them on failure. The game's traffic then goes over the
  chosen socket.
- **The odds** [design: our arithmetic]:
  - 256 mapped ports among 64,512: each probe hits with p ≈ 1/252.
  - The 9 s left before the checks give up allow about 2,175 probes (250 a second: under 25 KB/s at under 100 bytes each).
  - The chance that none hits is about (1 − 1/252)^2175 ≈ e^(−8.6), or 0.02%. Half the
    joins hit within about 175 probes (0.7 s).
  - A carrier NAT that hands out ports from a narrower block does better, as long as the
    block is inside the sweep.
- **Default:** on, unlike port prediction. It only ever probes the address of a player
  joining that game, for a few seconds, and the case it fixes (a home host, a guest on mobile
  data) is a common one. A setting can turn it off.
- **Not covered:**
  - Both NATs hard: each sprayed probe then opens a new mapping on the sprayer's own NAT. The
    peer relay and `nvf-relay` stay the answer there.
  - IPv6, which needs none of this.
- **Live** [observed: 2026-11-25, #4263]:
  - The setup:
    - A home host on port 47971, with no UPnP mapping (`NVF_COOP_PORTMAP=0`).
    - A laptop on a phone's mobile data: hard, on 31.94.30.4.
    - The test aid `NVF_COOP_TEST_BIRTHDAY_ONLY` on both sides, so the plain checks could
      not find the path.
  - 4 joins out of 4 connected through the punch, in 3.9, 2.1, 4.4 and 1.6 s. Each kept one
    extra socket, with 0 drops and 7–10 ms average lag.
  - Earlier joins with the host's port open by UPnP went direct in under 0.5 s, before the
    punch starts; without UPnP, plain checks failed.
- `Agent::punch_summary` gives the caller one line for its log saying what the punch did on
  that side.
- In `natsim` [design]: `NatKind::SymmetricRandom` maps each destination to a random port.
  Port-restricted against it connects, both ways round, the same model as the test above.

## Hosted relay (`nvf-relay`)

[design: #3795 section 9, option 2; issues #3880, #3908. Our own format, not any standard's]

A relay a player runs for friends whose networks cannot reach each other directly. It runs on a
VPS or a home server with one UDP port open (47961 by default), and never on the project's
servers. The game is pointed at it with `[Coop] Relay=host:port`.

- Every datagram starts with a 23-byte header:
  - `NVRL`;
  - version 2 (bumped from 1 in #3908 for the return-check cookie flow);
  - an op;
  - a 16-byte session id;
  - a side (0 host, 1 guest).
- The body follows.
- Ops:
  - 1 Bind (client to relay): claim or keep this side for the sending address. An initial claim
    or keepalive refresh from the active holder carries an empty body; a reply to a Challenge
    carries the 16-byte cookie.
  - 2 Bound (relay to client): the claim holds. The body is the refresh interval in seconds
    (u16, big-endian, 10).
  - 3 Refused: one reason byte. 1 means the relay is full. 2 means another address holds the side.
  - 4 Data: a payload of at most 1400 bytes. Accepted only from the proven active holder of
    that side; the relay refreshes the holder's timer and forwards it to the other side's active
    proven address, with the side byte naming where it came from. `Data` from an unproven or
    expired address is dropped silently and never claims a side (#3908).
  - 5 Challenge (relay to client): return check for an unproven address (#3908). The body is a
    16-byte cookie: HMAC-SHA256 (truncated to 16 bytes) over `"nvf-relay cookie v2\0"`, the
    sender's address (family byte, IP bytes, big-endian port), session id and side, keyed with a
    256-bit secret the relay rotates every 60 s (`nvf-relay` draws secrets from the OS random
    source; the previous secret stays valid for one rotation window so in-flight cookies still
    complete).
- Return check (#3908):
  - `Bind` from an already-proven active holder refreshes its timer and answers `Bound` with no
    extra round trip.
  - `Bind` from any unproven address (new claim, stale or forged cookie, or an address change
    after silence) gets `Challenge` without touching session state; only after echoing the cookie
    in `Bind` does the relay record the holder and answer `Bound` (or `Refused`). This adds at
    most one round trip per path and prevents spoofed UDP packets from turning the relay into a
    1:1 traffic reflector toward third-party addresses.
- The session id is the first 16 bytes of SHA-256 over `"nvf-relay session v1\0"`, the host's
  Noise public key and the guest's. Both sides already have both keys, so no new message is
  needed.
- A side not heard from for 30 s is let go. Only then may another address claim it (through the
  same Challenge/cookie return check), for example a guest that changed networks.
- Each side may send 256 KiB/s (a token bucket), and the relay holds 256 sessions. Both limits
  can be changed on the command line.
- Spoofed `Data` draws no reply; a `Refused` is 1 byte longer and a `Challenge` 16 bytes longer
  than an empty `Bind` (39 bytes total), and no session state is allocated until a cookie is
  echoed.
- The relay sees only Noise ciphertext. Someone who knows both keys and controls a reachable
  address could still claim a side first, which blocks the relay for that pair but cannot read or
  forge its packets and cannot reflect traffic to a spoofed third-party address.

In the game:
- The relay's address is a candidate of kind `relay` (`Kind::Relayed`), ranked below all
  others.
- Both sides pair with it like any remote address. Datagrams to it are wrapped, and datagrams
  from it are unwrapped by session before Noise sees them.
- When a relay pair is the best that has answered, the guest waits 2 s (not 200 ms) for a
  direct pair before choosing it.
