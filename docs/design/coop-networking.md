# Co-op networking: peer-to-peer without Steam (design spec, draft)

Status: draft for the owner, 2026-11-24 (#3795). It replaces the transport of the co-op
pull request #3723 (`engine/nvf-coop`): the way games find and reach each other. The game
protocol on top (`net.rs` messages, `rel.rs`, `repl.rs`) stays as it is. Built first on
branch `claude/coop-net-p1` (draft #3910) over #3723's whole head; brought onto main's
network core (#4531) by #4901, under its `seam::Transport`: §16 says what changed in that
port and which of the tests below were run again on it.

Tags as the charter asks: `[observed]` measured here, `[documented]` with a source,
`[design]` a choice this spec makes, `[hypothesis]` not yet checked.

---

## 0. In one page

**The owner's rule:** nv-forever.com's Cloudflare **introduces** players. It **never carries game
traffic** and never runs game servers [design, owner 2026-11-24]. Game traffic goes from player to
player only.

**The design:**

1. **A small matchmaking service on Cloudflare**, `coop.nv-forever.com`: a Worker plus one Durable
   Object per lobby. It hands out random lobby codes and passes a few small messages while
   players connect (addresses, public keys). After that it is idle until the lobby closes.
   Everything it holds lives in memory and is gone when the lobby closes.
2. **Our own NAT traversal**, a small version of WebRTC's ICE:
   - Each game collects every address it might be reached at: its LAN addresses, IPv6, the
     public address a STUN server sees, and a UPnP / NAT-PMP / PCP port mapping.
   - Both sides send probes to every pair of addresses at the same moment (hole punching).
   - The best pair that answers wins.
3. **Encryption and identity with the Noise protocol** (`snow` crate):
   - Every packet is authenticated and encrypted.
   - Every player has a key pair. The key replaces today's plaintext session token, so nobody
     can spoof or take over a slot.
   - The probes in step 2 *are* the Noise handshake, so a working path is already an
     authenticated one.
4. **When a direct link fails, another player relays.** A guest who cannot reach the host
   but can reach another guest goes through that guest. This extends the PR's
   `Msg::Relayed`. No server of ours is involved.
5. **LAN and direct join without the internet:** LAN broadcast discovery and typing `ip:port`
   (Tailscale, ZeroTier, port forwarding) keep working, encrypted the same way.
6. **Steam goes:** the `steamworks` crate, `steam_api64.dll`, the delay-load link flag in
   `build.rs`, Spacewar (app 480) and the Steam-account lobby codes.

**Why it is worth it:**
- Lobby codes stop exposing the host's IP address.
- Packets stop travelling in plain text.
- No closed DLL to redistribute.
- Players are not shown as "playing Spacewar".
- With nobody in the lobby able to relay, a lobby where both ends are behind hard NATs
  fails with a clear message, where the Steam relays used to save it. The owner decided
  (2026-11-24) that the project runs no relay. Section 9 covers what fills the gap instead.

---

## 1. What PR #3723 does today

From `engine/nvf-coop/src` on `local/coop-pr-3723` [observed, code read]:

| Part | What it does | Problem |
|---|---|---|
| `net.rs` | One UDP socket. The host is the hub for up to 5 guests. Magic `NVCO`, protocol version 137. A guest's `Hello{token, lobby, name}` names it across dropouts. | No encryption or authentication. Anyone who sees a token can take that guest's slot. Anyone can send a forged packet from a spoofed address. |
| `code.rs` | The code *is* the host's address: 4 bytes of IPv4 or a Steam account number, plus a check byte and the port, in Crockford base32. | Sharing a code on Discord or a stream gives away the host's home IP. No IPv6. A code never expires. |
| `reach.rs` | UPnP IGD opens the port; STUN via `stun.l.google.com`; `NVF_COOP_ADDRESS` overrides. | Only the host's side is opened. There is no hole punching, so a host behind a NAT whose router will not do UPnP cannot be reached. |
| `steam.rs` | The game starts Steam as **Spacewar (app 480)** and sends through Steam Networking Messages. | 480 is Valve's test app for developers. Players show as "playing Spacewar". It needs Steam running and `steam_api64.dll`, a closed DLL under the Steamworks SDK licence, beside a GPL program (`build.rs` delay-loads it). The build does not copy the DLL, so Steam silently falls back to UDP [observed: the 2026-11-24 build had no DLL beside the exe]. |
| `rel.rs` | Go-back-N reliable stream, resend 150 ms, items ≤ 1100 bytes, packets ≤ 1150 bytes. | Fine. It stays. |

**Traffic measured on this PC** [observed, 2026-11-24]:
- Setup: one host and one guest over loopback, Goodsprings, 12 actors in the area.
- Host to guest: about **12 KB/s** steady, and 25 KB/s just after the join.
- Guest to host: about **2.7 KB/s**.
- The join's snapshot peaks at about 110 KB/s, from `REL_PACKETS` in `net.rs` [documented in code].

Busier places will cost more [hypothesis: measure the Strip and a big fight, section 12].

---

## 2. Goals and non-goals

**Goals** [design]:
- G1. A friend anywhere joins with a short code and nothing else: no port forwarding, no Steam,
  no VPN, in the large majority of home networks.
- G2. Game traffic is peer-to-peer only. The owner's servers pass only introductions (section 0).
- G3. Every packet is encrypted and authenticated. Identities survive dropouts and restarts
  without a plaintext token.
- G4. A code does not reveal anyone's address. A player's address goes only to the players
  it connects to.
- G5. It works with no internet (LAN), and against a typed address (VPNs, forwarded ports).
- G6. When a link cannot be made, the game says why, and what will fix it, in plain words.
- G7. No closed-source runtime dependency. Permissive Rust crates only (deny.toml), no tokio
  inside the game unless chosen on purpose (`nvf-coop` is threaded and sans-async today).
- G8. Anyone can run the matchmaking service: its source is open, and the game takes another
  endpoint (`NVF_COOP_SIGNAL`).

**Non-goals:**
- Dedicated or hosted game servers.
- Relaying through the owner's infrastructure by default.
- Matchmaking with strangers or a public server browser (a later idea at most).
- Cheat prevention: the host is trusted, as today.
- Moving the host when it leaves (host migration).

---

## 3. The pieces

```
 ┌──────────── coop.nv-forever.com (Cloudflare) ────────────┐
 │ Worker: POST /v1/lobbies, GET /v1/lobbies/{code}         │
 │ Durable Object per lobby: WebSocket rooms, small JSON     │   introductions only:
 │ stun.cloudflare.com (Cloudflare's free STUN)             │   codes, keys, addresses
 └───────▲─────────────────────────────▲────────────────────┘
         │ WSS (setup, then idle)       │ WSS
   ┌─────┴─────┐   encrypted UDP    ┌───┴──────┐
   │   HOST    │◀══════════════════▶│ GUEST A  │
   │ nvf-coop  │◀════════╗          └──────────┘
   └───────────┘         ║ direct, or through A when C cannot reach the host
                    ┌────╨─────┐
                    │ GUEST C  │
                    └──────────┘
```

New modules in `engine/nvf-coop/src` [design]. Each is sans-IO where it can be, with the state
machines tested without sockets:

| Module | Job |
|---|---|
| `transport.rs` | The socket layer under `net.rs`. One UDP socket each for IPv4 and IPv6. Hands `net.rs` packets by `PeerId`, not `SocketAddr` (today's Steam stand-in addresses go away). |
| `noise.rs` | Noise handshakes and per-peer cipher states (`snow`). |
| `ice.rs` | Gathers candidates, runs the connectivity checks, picks a pair, sends keepalives, notices a dead path. |
| `stun.rs` | A STUN Binding request and response (RFC 8489): about 150 lines, hand-written, with no crate needed. |
| `portmap.rs` | UPnP IGD (today's `igd-next`), plus NAT-PMP and PCP (e.g. the `natpmp` crate, or hand-written: both are small). |
| `signal.rs` | The WebSocket client for the matchmaking service (`tungstenite`, blocking, on the network thread). |
| `code.rs` | Rewritten: random codes (section 5). The LAN and direct forms stay. |
| `lan.rs` | Today's broadcast discovery moved out of `net.rs`, now answering with the host's public key. |

---

## 4. Identity and encryption

**Keys** [design]:
- Each install has a Curve25519 static key pair, made on first co-op use.
- It is kept in `NVFOREVER_HOME/coop/identity.key`, never in the game folder (charter).
- Its public key is the player's identity in co-op. It replaces `new_token()` and
  `coop-token.txt`.

**Handshake:** Noise `XX` (`Noise_XX_25519_ChaChaPoly_BLAKE2s`) between guest and host.
- With the matchmaking service, each side already has the other's public key, so `IK` saves
  a round trip. `XX` is the general case and also covers LAN and typed-address joins.
- The host remembers a guest by its key. A guest coming back from a new address or a restarted
  game gets its old slot, as the token does today.

**Packets after the handshake** [design]:
- `NVCO | version | peer-session id (4) | nonce (8) | ciphertext(kind + fields) | tag (16)`.
- Use `snow`'s stateless transport mode with the explicit nonce, because UDP reorders and
  drops. A sliding replay window (as in DTLS/WireGuard, 2048 bits) drops repeats.
- The overhead is 28 bytes, so `rel::PACKET_BUDGET` drops from 1150 to about 1120 to stay
  under a 1200-byte datagram.

**Trust** [design]:
- Over the matchmaking service, the service hands each side the other's key, so trust in the
  key rests on the service (TLS) plus the code.
- Over LAN or a typed address: trust on first use, and the host's key fingerprint is shown in
  its lobby screen (a short word list). A LAN code can carry 4 bytes of the key's hash.
- **Join approval** (owner decision, section 13): the host gets "Courier wants to join:
  Accept / Decline", with Kick and Ban (by key) in the lobby screen.

**Licences:** `snow` is Apache-2.0/MIT [documented, crates.io], which fits deny.toml
[hypothesis: run `cargo deny`].

---

## 5. Lobby codes

**Form** [design]:
- 6 characters of Crockford base32 (today's alphabet and forgiving reading), shown `K7Q-M2X`:
  2³⁰ ≈ 1.07 billion codes.
- Random, made by the service, alive only while the host's WebSocket is open (plus a 2-minute
  grace for a restart).
- With fewer than 100,000 live lobbies at once, a random guess hits one about once in 10,000
  tries, and the service rate-limits code lookups to 10 a minute per IP and 1 a second per
  Durable Object.
- Join approval (section 4) is the real gate.

**Other forms** [design]:

| Form | Example | Meaning |
|---|---|---|
| Online | `K7Q-M2X` | Ask the matchmaking service. |
| LAN | `L-K7QM-2X9D` | Today's address code, used only on the same network. Broadcast finds the host, and the code carries the key hash. |
| Direct | `203.0.113.5:47960` or `[2001:db8::5]:47960` | For VPN and forwarded-port players. TOFU key. |

The game tells the three apart by shape. A code never holds an address that came from the
internet.

---

## 6. The matchmaking service (`coop.nv-forever.com`)

A new Worker, kept apart from the reports Worker: its own name, its own Durable Object class,
no D1, no R2 [design].

| Call | What it does |
|---|---|
| `POST /v1/lobbies` | The host sends `{protocol: 137, build, load_order_hash, max_players: 6, host_key, name}` and gets `{code, host_secret, stun: [...], expires}` back. 403 for a protocol the service has turned off (the reports Worker's build gates, reused). |
| `GET /v1/lobbies/{code}` | A guest's first check: `{protocol, load_order_hash, players, full}`. The game says "the host runs another load order" or "another version" **before** it tries to connect. |
| `GET /v1/lobbies/{code}/ws` | WebSocket. The host connects with `host_secret`, a guest with its key. The Durable Object passes these messages between the host and each guest: `join {name, key, candidates}`, `accept`, `decline`, `candidates {...}` (sent as they are found: trickle), `connected`, `reconnect`, `bye`. |
| Close | The host's `bye`, or its socket gone past the grace, ends the lobby. Every guest is told, and the code is free again. |

**What it stores:**
- Only the Durable Object's memory: the code, the keys, the open sockets and the candidates
  in flight.
- No D1 rows, no R2 objects. Invocation logs are off, as with reports [documented,
  NVForever-Data README §Privacy].
- The client's IP (`CF-Connecting-IP`) is used only to rate-limit and to add a public-address
  candidate (`srflx-tcp`, a weak hint). It is never written anywhere.

**Load** [hypothesis, to check on Cloudflare's price list]:
- Setup is about 10 small messages per player per join, plus a WebSocket held open while the
  lobby lives.
- WebSocket hibernation keeps an idle lobby's Durable Object asleep, so cost scales with
  joins, not playing hours.
- The game pings its lobby socket after 20 s with nothing sent (`signal_io.rs` `KEEPALIVE`):
  through a VPN (NordVPN) a socket idle for 60 s was closed (close 1006), and one pinging every
  20 s stayed open; without the VPN an idle socket stayed open past 90 s, so the close was the
  VPN's idle timeout, not the service's [observed 2026-11-25]. The service answers the pings
  itself; no Worker change was needed.
- This should stay inside the Workers Paid plan's included requests the reports already pay
  for.

**Self-hosting:**
- The Worker's source lives in a repository of its own, as every Cloudflare application of the
  project does (NVForever-Data, NVForever-Site) (owner, 2026-11-24). Phase 3 tells the owner
  what to set up there.
- The game takes `NVF_COOP_SIGNAL=https://…` (a `[Coop] SignalUrl` ini key).
- The wire format is written down in `docs/design/coop-signal-wire.md`.
- A small Rust stand-in implements the same protocol for tests, so CI needs no Node (section 12).

---

## 7. Getting through NATs

### 7.1 Candidates

Each game gathers these on its own socket (one per address family), ICE-style [design]:

| Kind | Where from | Notes |
|---|---|---|
| `host` | The machine's LAN IPv4 and global IPv6 addresses | IPv6 is often reachable directly with no punching. That is common on mobile and some ISPs [hypothesis: measure in the field]. |
| `srflx` | STUN Binding to 2 servers: `stun.cloudflare.com:3478` (free and unlimited [documented, Cloudflare TURN FAQ]) and a second (`stun.l.google.com:19302`) | Two servers show whether the NAT keeps one public port for every destination (endpoint-independent: punchable) or a new port per destination (symmetric: hard). |
| `mapped` | UPnP IGD / NAT-PMP / PCP asks the router for a port (today's `reach.rs`, for every player, not just the host) | Removed on leave. A setting turns it off. |
| `peer-relay` | Another player in the lobby (section 8) | Last resort. |

### 7.2 Connectivity checks (hole punching)

1. Over the service, the host and guest swap candidates as they find them.
2. Each makes pairs (local × remote) and sorts them: IPv6 direct, LAN, mapped, srflx. Then it
   sends a probe to every pair every 20 ms, round-robin, for up to 10 s. The two sides start
   within a few ms of each other, because the service sends `connected`/`go` to both at once.
3. A probe is a Noise handshake message (the first is `XX` message 1) carrying a check id.
   An answer from the right key on a pair makes that pair **valid**.
4. The guest names the best valid pair (lowest latency within one priority class). The other
   pairs stop.
5. Keepalives go every 1 s, because many NATs forget a UDP mapping after 30 s; today's
   heartbeat at `SEND_HZ` already covers this while playing. While loading (the `Link`
   thread keeps running), the 1 s keepalive carries on.

**Symmetric NATs** [design, phase 5]:
- When STUN shows one side is symmetric and the other is not, the easy side probes the hard
  side's ports near its last seen mapping.
- With both sides symmetric, the "birthday" method: one side opens 256 sockets, the other
  probes 256 random ports. This is the method Tailscale has described publicly
  [hypothesis: check their write-up before quoting numbers].
- It is optional, behind a setting, because it looks like a port scan to some firewalls.

### 7.3 When the path breaks

- Today's `LOST_AFTER` (4 s) and `GRACE` (120 s) stay.
- When a path is lost, the guest asks the service for a `reconnect`. Both sides gather again
  (a new Wi-Fi, a new IP) and rerun 7.2.
- The Noise identity gives the guest back its slot, as the token does today.
- If the service cannot be reached, the guest keeps sending to the last valid pair and any
  `host`/`mapped` candidates, as today's Hello loop does.

---

## 8. When no direct path exists: other players relay

The owner's rule rules out a relay of ours by default. The fallback is the players themselves
[design]:

- In a lobby of 3 or more, a guest C that cannot reach the host H may reach a guest A that can.
  The service tells C which players are connected. C runs checks against A's candidates too,
  and if A is reachable, A forwards C's packets to H and back.
- The PR already has the pieces: `Session::open_relay`, `Msg::Relayed` with sub-channels, and
  the host counting a relayed game as a guest of its own (split screen's player 2 uses them
  today) [documented in `net.rs`].
- The Noise session stays **end to end, C to H**. A only sees ciphertext inside `Relayed`, so
  A cannot read or forge C's traffic.
- The cost to A is C's traffic both ways, about 15 KB/s in Goodsprings [observed, section 1].
  A gets a "relaying for C" line in the lobby screen and a setting to refuse.
- **It cannot help** a 2-player lobby, or a lobby where no one reaches both sides. Then the
  game says so (section 10).

**How often this happens** [hypothesis]: direct UDP plus hole punching is widely said to reach
most home-to-home pairs, with the rest mostly double-hard NATs (CGNAT on both mobile ends,
some campus networks) and networks that block UDP. Phase 2 measures it with real players
(section 12) before anyone decides whether a paid relay is wanted (section 9).

---

## 9. Choices for the gap (owner decision)

Ranked by how well they fit the owner's rule:

1. **Accept the gap.** Show the fix list (section 10). Players on hard networks use Tailscale or
   ZeroTier, or forward a port. Costs nothing.
2. **Players run relays themselves.** A tiny standalone `nvf-relay` binary (the same Noise
   forwarding as section 8) anyone can run on a VPS or a home server with a port open, typed in
   as `[Coop] Relay=host:port`. It is not the owner's server.
3. **A relay that some players' own clients provide**, opt-in and public: a volunteer
   `nvf-relay` list, like Tor relays. More work, and abuse questions. Not recommended now.
4. ~~Cloudflare TURN, paid by the project, opt-in~~ **Rejected by the owner (2026-11-24: "No
   relay of ours").** Kept for the record: $0.05 per GB from Cloudflare to the client, 1,000 GB
   a month free [documented, Cloudflare TURN FAQ]. At about 15 KB/s per relayed guest, that
   would be about 18,000 relayed guest-hours a month free.

**Decided (owner, 2026-11-24): no relay run or paid for by the project.** The design is 1 plus 2:
the fix list, peer relay (section 8) and `nvf-relay` for players who host their own. No phase
adds a project relay.

---

## 10. What players see

**Lobby screen (host):**
- The code.
- A line per player: connecting / direct / LAN / via <player> / lost.
- Its ping.
- Join approval prompts, Kick, Ban.

**When a join fails** (a reason per failure, own words, not transcribed):
- "Couldn't reach the host from your network" (both NATs hard). Suggestions:
  - try with a third player in the lobby;
  - ask the host to turn on UPnP on their router or forward UDP port 47960;
  - use Tailscale and join by address.
- "Your network blocks UDP" (no STUN answer at all): school or work networks.
- "The host runs a different load order or version" (from `GET /v1/lobbies/{code}`).
- "Code not found or the lobby has closed".

**Settings** (follow `docs/design/settings-page.md` §0: place in `SECTIONS`, lists of ≤ 7 values,
greyed when not in co-op):

| Row | Values |
|---|---|
| Co-op connection | Automatic / Direct only / LAN only |
| Open router port automatically (UPnP) | On / Off |
| Relay for other players | On / Off |
| Ask before players join | On / Off |

**Launcher:** the Windows Firewall button stays (it is also needed for direct UDP).

---

## 11. What gets removed

When the new path ships (phase 4) [design]:
- `engine/nvf-coop/src/steam.rs` and the `steamworks` dependency.
- `nvforever/build.rs`'s `/DELAYLOAD:steam_api64.dll` and `delayimp.lib` lines.
- `NVF_COOP_STEAM`.
- `code::Host::Steam`, the Steam check byte and the Steam stand-in addresses in `net.rs`.
- `reach::stun()`'s Google-only STUN (it moves into `stun.rs` with two servers).

The CO-OP launcher page's Steam mentions go too.

---

## 12. Testing

| Level | How |
|---|---|
| Unit | `stun.rs` encode and decode against RFC 8489's test vectors. `noise.rs` round trips, replay window, reordering. `ice.rs` pair sorting and its state machine on a fake clock. `code.rs` forms. |
| Simulated NAT | A test-only `natsim` in `nvf-coop`: a userspace "router" between sockets on loopback with each NAT kind (full cone, restricted, port-restricted, symmetric, UDP blocked). The tests assert which pairs connect, directly or through a peer. This runs in `cargo test`. |
| Service | The Rust stand-in service in-process, plus a conformance test that sends the same message script to the Worker under `wrangler dev` (NVForever-Data's tests, as `npm run dev:ingest` does for reports). |
| Smoke | A `[[scenario]]`: a host and a guest on one PC through the stand-in service on loopback, expecting the guest in as player 2 and a `lan` or `direct` path. This is the duo run of 2026-11-24 made into data. Built 2026-11-25 as `coop-online-local-duo-browse` (tools/smoke/scenarios.toml): the service runs in the game (`NVF_COOP_SIGNAL=local`, `nvf-signal-server` in-process) and the guest is a session with no window in the same process (`NVF_COOP_TEST_GUEST`), since the smoke runner starts one game per scenario; the guest goes through the service, join approval, the checks and the Noise session as a second game would. |
| Field | Phase 2 build: a `co-op connect:` log line per join with the pair kind chosen, NAT kinds, and time to connect. Gathered **only** through the player reports pipeline when a player opted in (charter: player reports), never from the service. |
| Bandwidth | Repeat the section 1 measurement on the Strip, in a big fight, and in a 6-player lobby. |

---

## 13. Phases

Each phase is its own pull request with an issue first (CLAUDE.md), once this leaves draft.

| Phase | Work | Done when |
|---|---|---|
| 1 | `transport.rs` under `net.rs`: all socket and Steam I/O in one place, IPv4 and IPv6 sockets on the host's port (IPv6-only, `socket2`), IPv4-mapped addresses taken as IPv4. Peers stay keyed by `SocketAddr`; `PeerId` moves to phase 3, where a peer's path can change. No change in behaviour over IPv4. Built 2026-11-24 on `local/coop-net-p1`. | `nvf-coop` tests and the duo smoke pass, unchanged. |
| 2 | `noise.rs`: identity keys, `XX` on LAN and direct joins, encrypted packets, token removed. Protocol 138. Built 2026-11-24: session indices let a guest keep its session from a new address; a host started again answers unknown sessions with an unauthenticated hint (`0xE4`) that a guest takes only after 1 s without hearing it; a game behind a guest (split screen) has its own session with the host through the guest, and hears the host leave by a plain Bye from its loopback relay. Key in `coop-identity.key` in the home. `rel::PACKET_BUDGET` 1150 to 1110. | Duo smoke passes encrypted; forged and replayed packets are dropped in tests. |
| 3 | Matchmaking Worker plus stand-in, random codes, `stun.rs`, `ice.rs`, `portmap.rs` for every player. Built 2026-11-24: `crates/nvf-nat` (#3798) and `crates/nvf-signal` (#3800) by another session; the Worker in its own repository, live at coop.nv-forever.com; the game side in `engine/nvf-coop/src/online.rs` and `signal_io.rs`. Differences from §7.2: the checks' probes are packets of their own (`0xE5`, `0xE6`), not Noise messages; a guest instead takes a Noise session only from the host key the service gave it (pinned), so a forged probe answer wastes a try and nothing more. Join approval and PCP/NAT-PMP/UPnP for every player wait for phase 4; a host whose service is unreachable falls back to the old address code. | Two PCs on different home networks join by code with no forwarding; `natsim` matrix green. |
| 4 | Peer relay (section 8), failure screens, settings rows, Steam removed (section 11). Built 2026-11-24 (local branch `local/coop-net-p1`): Steam removed (with it the `paste` advisory); every player asks its router for the game's port (`engine/nvf-coop/src/portmap.rs`: PCP, then NAT-PMP, then UPnP IGD by `igd-next`; the router's address by `netdev`), a mapped candidate; the peer relay (`online.rs` runs `nvf_nat::relay::Planner` and `Policy`; relay checks are probes `0xE7`/`0xE8`; a helper passes the relayed player's sealed packets in `Msg::Relayed` on sub-channels from `0x80`, so the relay sees ciphertext only; a relayed player silent for 60 s frees its slot); join approval (Let In / Turn Away in the Co-op Lobby, 120 s to answer; the guest waits 130 s); the settings rows (Gameplay › Co-op, `nvf_coop::prefs`). Verified live through coop.nv-forever.com: three games, one forced by a test aid (`NVF_COOP_TEST_NO_DIRECT`) to relay through another guest. Also built 2026-11-24: Kick and Ban (by key, while the lobby lasts), the host's line per guest with its path and ping (the reliable stream's round trip), the "your network blocks UDP" message, every adapter's address as a candidate (a host on a VPN offered only the tunnel's), the co-op rows read before a command-line host or join, and the relay a player hosts (`nvf-relay`, §9 option 2: `[Coop] Relay=host:port`, `[Coop] SignalUrl=`; a session per pair with a stand-in address in 240.0.0.0/4) with the `co-op connect:` line (§12). Tested on two machines on one home network through coop.nv-forever.com: join by online code (LAN path), a guest forced to relay through another guest, Direct only and LAN only, and a guest forced through the desktop's `nvf-relay`; each with 0 desyncs. Still open: the replication's desyncs on a few placed references with 3 or more players (`repl.rs`, from #3723; 12 to 17 a run), and a host on a VPN (hard NAT, no router port) reachable from other networks only through a relay or port prediction (`ice::Config::predict`, off by default). Different networks, 2026-11-25 [observed]: a home host (one public port for every server, filtering by address and port) and a laptop on mobile data (a carrier NAT, a new port per server) found no direct path, with no player to relay. The birthday punch (#4263, `nvf_nat::ice`, docs/spec-notes/nat-traversal.md) is wired into the game: the hard side opens 256 more IPv4 sockets (`transport.rs` `open_extras`), probes from each (`send_from`), answers a probe from the socket it came in on (`recv_via`), and keeps the chosen one as the way to that peer (`close_extras`); a host on the hard side keeps the socket the guest's handshake came in on (`pin`); a relay's checks never punch. Run again between the two networks on build 1a8d04168 [observed]: the laptop on mobile data (hard) joined the home host (easy) three times out of three, `path=punched` (a server- or peer-reflexive pair) in 280 to 372 ms, lag 9 to 12 ms average, 0 drops; the plain checks won each time, before the birthday punch's 1 s, so it opened no sockets. So the home router filters by address only [hypothesis: the host's probes to the laptop's STUN-seen ports opened it to that address, and the laptop's new port came in as peer-reflexive]; the same pairing failing earlier fits a carrier NAT that sometimes maps toward the host from another public address than STUN saw [hypothesis]. The birthday punch (an easy side filtering by address and port against a hard side; two hard NATs it cannot help) `NVF_COOP_TEST_BIRTHDAY_ONLY=1` turns the plain checks off on either side so that only it can find a path. Run live 2026-11-25 on a392fa875 with the aid on the laptop (hard, mobile data) against the home host (easy, 1a8d04168, no aid) [observed]: 256 more sockets opened, `path=punched nat=hard peer_nat=easy` in 1379 ms, the path kept on one extra socket, joined with 11 ms average lag and 0 drops. That proves the guest's side (`open_extras`, `send_from`, `recv_via`, `close_extras`); the host had a UPnP mapping open on that run, so its spray was not needed. Then with no router mapping (host on port 47971, `NVF_COOP_PORTMAP=0`) and the aid on both sides [observed]: 4 joins out of 4, `path=punched nat=hard peer_nat=easy` in 1603 to 4443 ms, each on a kept extra socket, lag 7 to 10 ms average, 0 drops; the host let the guest in and found its path 4 s later, the time a spray hit takes. The birthday punch is proven live from a home network to mobile data. The host logs no note of its own for the spray. The connect line's `peer_nat` is read from the candidates the service passed on (`report::Nat::from_candidates`). | 3-player test with one guest forced to relay; no `steamworks` in `cargo tree`. |
| 5 | Optional: symmetric NAT tricks, `nvf-relay` for self-hosters, join approval polish. | By field numbers. |

---

## 14. Other ways considered

| Option | Why not (or not now) |
|---|---|
| **iroh** (n0): QUIC, key identities, hole punching, relays | Strong and complete, but hole punching needs a relay server the peers connect to [documented: iroh docs, relays and hole punching]. A self-hosted iroh-relay needs a host with a public IP, DNS and TLS, ideally its own ports 80 and 443 [documented: iroh FAQ; a third-party write-up]. That is a server of ours carrying traffic when punching fails, against the owner's rule. It also brings tokio and QUIC streams in place of `rel.rs`. Worth a second look if the owner ever runs a VPS relay. |
| **WebRTC data channels** (`str0m`, sans-IO) | Standard ICE, DTLS and SCTP, and the same signalling Worker would do. Heavier: SCTP and DTLS overhead, and much more code to trust. Its ICE is the thing this spec rebuilds in a smaller form. A fair alternative to `ice.rs` plus `noise.rs` if writing our own proves troublesome. |
| **Valve GameNetworkingSockets (open source)** | C++ with protobuf and OpenSSL to build. Its relay network (SDR) is Valve's only. |
| **Epic Online Services P2P** | A closed SDK and DLL again, tied to Epic. The same problem as Steam. |
| **A real Steam app id** | NVForever cannot have one without being on Steam, and the closed DLL problem stays. |
| **Hosted game servers** | Ruled out by the owner. |

---

## 15. Decisions for the owner

1. ~~The gap~~ **Decided 2026-11-24: no relay of ours.** The fix list, peer relay and
   `nvf-relay` for self-hosters (section 9).
2. ~~Join approval on by default?~~ **Decided 2026-11-24: on** for online codes (players on the
   host's network join at once); the Ask before players join row turns it off.
3. ~~Where the Worker lives~~ **Decided 2026-11-24:** a repository of its own, like the other
   Cloudflare applications.
4. ~~Peer relay default~~ **Decided 2026-11-24: on** (the Relay for other players row).
5. ~~UPnP default~~ **Decided 2026-11-24: on** (the Open router port automatically row).

## Sources

- Cloudflare TURN FAQ and pricing: https://developers.cloudflare.com/realtime/turn/faq/ ,
  https://developers.cloudflare.com/realtime/pricing/
- iroh relays and hole punching: https://docs.iroh.computer/concepts/holepunching ,
  https://docs.iroh.computer/about/faq
- STUN: RFC 8489. ICE: RFC 8445. Noise: https://noiseprotocol.org/noise.html
- NVForever-Data README (reports service privacy and plan).

---

## 16. On main: the port onto the network core (#4901, 2026-11-28)

#3723 was split into smaller pull requests; its network core (#4531) is on main with a
seam for this transport (`engine/nvf-coop/src/seam.rs`: `Transport`, by `SocketAddr`, and
`Admission`). Draft #3910 could not be rebased (its branch is #3723's whole head), so its
networking was ported onto that seam [design]:

- `stack.rs` is the transport the session is given: the sockets (`sockets.rs`, the
  branch's `transport.rs`), Noise (`noise.rs`, unchanged), the online code's service and
  checks (`online.rs`, `signal_io.rs`), port mapping (`portmap.rs`, `reach.rs`), and the
  hosted and peer relays. `net.rs` sends plain messages to an address and takes them from
  one, as on main; the stack seals them. A guest joining by online code joins the stand-in
  address `stack::ONLINE_HOST`, and the stack sends to wherever the checks found the host
  (direct, hosted relay, or through another guest).
- The peer relay's frames are the stack's, not the game protocol's: a helper sends the
  relayed player's sealed packets to the host as a frame on its own Noise session
  (plaintext `NVRF`, a sub-channel byte from `0x80`, the packet), in place of the branch's
  `Msg::Relayed`.
- The token stays in the Hello, as on main, but it is the key's (`stack::token_of`: the
  public key's first 8 bytes), and the host's admission (`stack::KeyAdmission`, replacing
  `SharedSecret` for real games) lets in only the token of the key the sender's session
  proved (`Admission::admits_from`). Ban is by key (`Admission::ban`, `Session::kick`,
  `Msg::Kicked` kind 40). Protocol version 140 (main's 139 + 1).
- The game side is on main's Co-op screens (`engine/nvforever/src/coop/connect.rs`,
  `engine/nvf-play/src/lobby.rs`: Let In / Turn Away, Kick, Ban) and the settings rows of
  the branch (Gameplay › Co-op, `coop/net_settings.rs`).
- Not ported in #4901, then ported in §17: the lobby browser, a host's online code kept
  across a load's restart (`host_again`), the per-guest path and ping line, the join
  password, and the load order hash. Split screen's relay waits for split screen itself
  (#4845). The birthday punch's socket side is in (`sockets.rs`, run by `stack.rs`); its
  game-side test aids came with `online.rs` unchanged.

What was run again on the port [observed, 2026-11-28, this PC only]: `cargo test -p
nvf-coop` (the branch modules' own tests, and `stack.rs`'s: two games joining over Noise
on loopback, a Hello whose token is not its key's turned away, a banned key kicked and told
so when it comes back, a plaintext Hello never passed up), the engine and root tests and
clippy, cargo deny, and the smoke scenarios the change calls for (77 pass, 2 known; one,
`settings-heading-pointer`, lost its click with three games on the machine and passed run
alone). Single player is unchanged with co-op off: nothing opens a socket until a lobby is
created or joined. **Not run again**: every
two-machine test of §13 phase 4 (online code through coop.nv-forever.com, the peer relay,
the hosted relay, Direct only / LAN only, the birthday punch between a home network and
mobile data). Those results are the branch's (build 1a8d04168 and a392fa875), not this
port's, and the port needs one two-machine run through the service before co-op is
called working over the internet on main.

## 17. On main: the rest of the branch's networking (2026-11-28)

The follow-ups §16 listed, ported onto main's seam and Co-op screens [design]:

- **The lobby browser** (docs/design/coop-lobby-browser.md, its "On main" note): the host's
  Visibility, Title and Password; the Join screen's Code and Browse tabs; favourites;
  reports. Game side `engine/nvforever/src/coop/browse.rs`; screen `engine/nvf-play/src/lobby.rs`.
- **The join password and the load order hash**: a host's online lobby is made with its
  listing, its password (hashed, `nvf_coop::online`) and the load order's hash
  (`coop/connect.rs` `set_load_order`, FNV-1a of the plugins' names in order); a guest's
  join by online code sends its own hash and the password typed, and the service and the
  checks refuse a mismatch before the host hears of it.
- **The per-guest path and ping line**: `Session::paths` gives each guest's address and the
  reliable stream's round trip (`rel.rs` measures it on items sent once, RFC 6298's
  smoothing); the host's lobby list says "(direct, 25 ms)" and the like
  (`stack::Path::of`, the words of §16's `Path`).
- **A lobby kept across a load's restart** (`host_again`): a host's run that a load restarts
  puts its lobby in the new run's environment (`NVF_COOP_HOST`, `NVF_COOP_CODE`,
  `NVF_COOP_SECRET`, the host's choices; the secret and the password never go to a file)
  and lets its link go without a goodbye (`connect::hand_over`, at the handoff's cover,
  `crate::handoff::Handoff::leaving`). The new run hosts on the same port with the same key,
  and takes its online code back from the service with the secret (`Stack::go_online_again`,
  `Online::host_again`); an address code is kept as it was. The guests' sessions start again
  by themselves: the host started again answers their data with the unknown-session hint
  (`noise.rs`). A new run that fails leaves this run hosting again. `NVF_COOP_HOST=<port>`
  set by hand opens a lobby on that port at start (the smoke scenario's way in).
- **Test aids** (`coop/test_service.rs`): `NVF_COOP_SIGNAL=local` runs the matchmaking
  service in the game (`nvf-signal-server`), and `NVF_COOP_TEST_GUEST=<name>` joins the
  host's online code from a thread with a key of its own, so one launch checks the online
  path offline (smoke `coop-online-local-duo-browse`).
- **Not ported**: split screen's relay (split screen is not on main; #4845 brings it).

What was run [observed, 2026-11-28, this PC only]: `cargo test -p nvf-coop` (153, the round
trip's test with them), the nvforever co-op tests (79: the browser's rows and listing, the
favourites file, the lobby's environment there and back, the load order hash), the nvf-play
lobby tests (12: the host's choices, the Browse tab's sort, filters, row, Report and fit),
the whole engine test suite, engine clippy, engine cargo deny, the smoke runner's tests, and
the smoke scenarios the change calls for (35 pass, 8 skipped for want of `NVF_DLSS_DIR` or a
lighting home), the new `coop-online-local-duo-browse` among them: the service in the game,
a public locked lobby, the test guest let in with the password, the Browse tab listing,
opening, sorting and reporting it. `physics-rate-fps` failed twice, the second time run
alone: the game held 49 and then 57 fps before the scenario's frame hold began, with
another session's build running on the machine, so Match FPS stepped down early; the
change touches that run only by noting the load order [hypothesis: machine load; to run
again on a quiet machine]. **Not run**: a load's restart with real guests in the lobby, and
any two-machine run through coop.nv-forever.com.

