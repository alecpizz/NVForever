# Co-op matchmaking: the wire format

Status: 2026-11-24, wire version 1. This is the contract between the game and the matchmaking
service at `coop.nv-forever.com` (#3795 section 6). The service introduces players and
passes keys and addresses. It never carries game traffic.

The same contract exists in code, in `crates/nvf-signal`:
- `wire.rs` has the types.
- `standin.rs` is an in-process service that the tests run against.

The Worker lives in a repository of its own and must behave like the stand-in. When this file
changes, the Worker, `wire.rs` and `standin.rs` change with it. A change clients cannot ignore
bumps `wire`. [design throughout]

## REST

Every non-2xx answer has the body `{"error": "<code>", "message": "<text for the log>"}`.

### `POST /v1/lobbies`: create a lobby (the host)

```json
{"wire":1,"protocol":138,"build":"0.8.1","load_order_hash":"…","max_players":6,
 "host_key":"<64 lowercase hex>","name":"Courier"}
```

- 200: `{"code":"K7QM2X","host_secret":"…","stun":["stun.cloudflare.com:3478","stun.l.google.com:19302"],"expires":1764000000}`.
  - `code` is 6 random characters of Crockford base32. The game shows it as `K7Q-M2X`.
  - `host_secret` is at least 128 random bits.
  - `expires` is in unix seconds. A lobby whose host socket never opens is gone after it
    (grace: 120 s).
- 400 `bad_request` (not this shape, a bad key, `max_players` outside 2..6) or `wire_version`.
- 403 `protocol`: this game version is turned off.

### `GET /v1/lobbies/{code}`: look a code up (the guest, before connecting)

- The code is accepted in any typed form: dash or not, any case, with `O`, `I` and `L` read as
  `0`, `1` and `1`.
- 200: `{"protocol":138,"build":"0.8.1","load_order_hash":"…","players":2,"max_players":6,"full":false}`.
  The game compares `protocol` and `load_order_hash` with its own and explains a mismatch
  before it tries anything.
- 404 `not_found`.
- 429: the rate limit. A zone rate-limiting rule on `nv-forever.com` allows 10 requests per
  10 s per IP across every `/v1/lobbies` path, the socket upgrade included. Past that, it blocks
  the IP for 10 s. Its 429 is Cloudflare's own plain-text answer with `Retry-After`, not the
  JSON error body: a client treats any 429 as "wait `Retry-After` seconds, then retry" and does
  not parse the body. The Worker's own limit (10 a minute on lookups and creates) is
  approximate per location and only a soft first check; its 429 is the JSON error body
  `rate_limited` with `Retry-After: 60`. One lobby also answers at most one info lookup a
  second (#3795 section 5), whoever asks; past that, the same JSON body with `Retry-After: 1`.
  The stand-in has no limit.

### `GET /v1/lobbies/{code}/ws`: the lobby socket

- This is a WebSocket upgrade, or 404 `not_found`.
- Each frame is a text frame holding one JSON object with a `"type"`.
- The first frame decides who the socket is: `host` or `join`.

## Socket messages, game to service

| type | From | Fields | Meaning |
|---|---|---|---|
| `host` | host | `wire`, `host_secret` | I am this lobby's host. Again after a reconnect: a new socket replaces the old one. |
| `join` | guest | `wire`, `name`, `key`, `candidates` | Let me in. `key` is 64 lowercase hex. |
| `accept` | host | `peer`, `candidates` | Let this guest in. My candidates so far. |
| `decline` | host | `peer`, `reason` | Turn this guest away. |
| `candidates` | either | `peer` (host only, optional), `candidates` | Found more (trickle). From the host, no `peer` means every accepted guest. |
| `connected` | guest | `path` | My path is up: `direct`, `lan`, `relay` (through another guest) or `hosted-relay` (through `nvf-relay`), at most 16 characters. For the host's lobby screen; only `direct` and `lan` make the guest a helper. |
| `reconnect` | guest | none | My path was lost. Start the checks again. |
| `relay_ask` | guest | `helper`, `candidates` | I can't reach the host: ask that guest to relay for me. These are my candidates, for its checks. |
| `relay_answer` | guest | `for`, `accept`, `reason`, `candidates` | My answer to `for`'s request. Accepting, I send my candidates. Refusing, `reason` is `off`, `busy` or `no_direct_path`. |
| `bye` | either | none | Leaving. From the host, this ends the lobby. |

A candidate is `{"kind":"host"|"mapped"|"srflx"|"prflx"|"relay","addr":"203.0.113.5:47960"}`, or
`"[2001:db8::5]:47960"` for IPv6. `relay` is a relay a player hosts (`nvf-relay`, issues #3880
and #3908; UDP wire version 2 with a Challenge/cookie return check before forwarding, specified
in `docs/spec-notes/nat-traversal.md`, "Hosted relay"). A receiver skips kinds it does not know.

## Socket messages, service to game

| type | To | Fields | When |
|---|---|---|---|
| `hosting` | host | `code`, `peers` | After a good `host`. `peers` lists the guests still in the lobby. |
| `waiting` | guest | `peer` | After a good `join`. `peer` is the guest's number (1, 2, …, never reused in a lobby). |
| `join` | host | `peer`, `name`, `key`, `candidates` | A guest asks. |
| `accepted` | guest | `host_key`, `host_name`, `candidates` | The host accepted. |
| `declined` | guest | `reason` | The host declined. The socket closes. |
| `candidates` | either | `peer` (to the host), `candidates` | Passed on. A guest's candidates go only after it is accepted. |
| `go` | both | `peer` (to the host) | Start the checks now. It goes to both in the same instant, right after `accepted` and after each `reconnect`. |
| `reconnect` | host | `peer` | That guest lost its path. A `go` follows. |
| `connected` | host | `peer`, `path` | That guest reported its path. |
| `left` | host | `peer` | That guest said `bye` or its socket closed. |
| `helpers` | accepted guests | `peers` | The other guests that could relay. Sent to every accepted guest whenever that set changes, to a guest right after its `go` on acceptance, and to a guest after its own `reconnect`. |
| `relay_ask` | guest | `from`, `candidates` | Another guest asks this one to relay. |
| `relay_answer` | guest | `helper`, `accept`, `reason`, `candidates` | A helper's answer. |
| `closed` | anyone | `reason` | `host_left`, `expired`, `replaced` or `bye`. The socket closes. |
| `error` | anyone | `error`, `message` | See below. |

Errors:
- These close the socket, because they answer the first frame: `bad_secret`, `bad_key`,
  `wire_version`, `full`, and `host_away` (the host is reconnecting; try again).
- These leave the socket open: `bad_message`, `unknown_peer`, `not_accepted`.

## Peer relay (#3795 section 8)

Guest C cannot reach the host, so the service is C's only channel. The relay exchange goes
guest to guest and the host takes no part in it.

- A guest becomes a **helper** when its `connected` path is `direct` or `lan`. It stops being
  one on `reconnect` or when it leaves. Each change sends `helpers` to every accepted guest,
  listing the others.
- A guest also gets the current `helpers` right after its `go` when it is accepted, and after
  its own `reconnect`, so one accepted after the last change still knows the set.
- C sends `relay_ask` to a helper. The service passes it on as `relay_ask` with `from`.
- The helper answers with `relay_answer`. The service passes it to C, with `helper` naming who
  answered.
- Both guests must be accepted, and they must be different guests; otherwise the answer is
  `unknown_peer`.
- After an accept, C and the helper run connectivity checks against each other's candidates.
  The forwarding is the game's: it is end-to-end encrypted, C to host.

## Lobby browser (wire 2)

[design: NVForever `docs/design/coop-lobby-browser.md`; the owner's decisions of 2026-11-25]

The service takes wire versions 1 and 2. A wire-1 game works as before, and its lobbies stay
private. Wire 2 adds the following.

**Create** takes an optional `listing`:
`{"public": true, "title": "…", "place": "…", "friendly_fire": false, "plugins": 14, "locked": true}`.
- `title` is cut to 40 characters and `place` to 32. A text in all capitals is lowered.
- A text the word filter blocks becomes empty. An empty title shows as "<host name>'s game".
- `locked` says a password will be set. Until the host's first `password`, every join is
  declined `bad_password`.

**Look-up** (`GET /v1/lobbies/{code}`) gains `locked`.

**Socket messages:**

| Message | From | Fields | Meaning |
|---|---|---|---|
| `listing` | host | `public`, `title`, `place`, `friendly_fire` | The listing changed. `public: false` takes the lobby off the browser at once. The game may send it whenever the place changes; the service updates the browser at most every 5 s. |
| `password` | host | `hash` | The password as hex SHA-256 over `lobby-password v1`, a zero byte, the compact code, a zero byte and the password's UTF-8. An empty `hash` removes it. Any lobby, private or public, may have one. |
| `join` | guest | `password` (optional) | The same hash of what the player typed. A wrong one gets `declined` `bad_password`. |

**Password lockout:** after 3 wrong passwords from one IP within a minute, that IP's socket
upgrades to that lobby get a 429 with `Retry-After: 60`.

**`GET /v1/browse`**: public lobbies.
- Query:
  - `protocol`;
  - `load_order` (a load-order hash);
  - `q` (search in title, host name and place, at most 32 characters, case-insensitive);
  - `empty=0` (hide lobbies with only the host);
  - `full=0`;
  - `cursor`.
- The answer:
  `{"lobbies": [row…], "cursor": "100" | null, "total": n}`, newest first, 100 rows a page.
- A row:
  `{code, title, host_name, host_fp, place, players, max_players, locked, friendly_fire, plugins, region, km, age, protocol, build, load_order_hash}`.
  - `host_fp` is the first 16 hex digits of the host's key, for favourites.
  - `region` is "continent/country" as Cloudflare places the host's socket.
  - `km` is the distance between where Cloudflare places the host's socket and the browsing
    request, rounded to 250 km. It is `null` when Cloudflare gives no location.
  - Rows carry no addresses.
- A lobby is listed while it is public, its host socket is open and fewer than 5 IPs have
  reported it.
- At most 2 public lobbies per IP. A third stays private, and its host gets an `error`
  `too_many_public`.
- One browse every 2 s per IP (`browse_rate_limited`, `Retry-After: 2`).

**`POST /v1/lobbies/{code}/report`**: body `{}` or `{"reason": "…"}`, answered `{"ok": true}`.
- Each IP counts once.
- At 5 reports, the lobby leaves the browser. It can still be joined by its code.
- Reports are kept only while the lobby lives.

**The Directory:** one Durable Object holds every public lobby's row in memory.
- A lobby renews its row every 30 s, and a row not renewed for 90 s is gone.
- Nothing is stored. A restarted Directory fills again within 30 s.
- IPs reach the Durable Objects only hashed, with a fixed label.

The stand-in (`crates/nvf-signal`) and `nvf-signal-server` serve the same calls. They have no
word filter, no rate limits, `km: null` and an empty `region`.

## Lifetimes

- A lobby lives while its host socket is open.
- When the host socket closes, the lobby and its guests wait 120 s for a new `host` with the
  same secret. Then every socket gets `closed` `expired` and the code is free.
- `bye` from the host ends the lobby at once: `closed` `host_left`.
- A guest's socket closing is a `left` to the host.
- `full` counts the host: players = 1 + guests. A declined guest does not count.

## What the service keeps

- Only memory: the code, the secret, the keys, the sockets and the candidates in flight.
- No database rows, no stored objects, invocation logs off.
- The client's IP is used only for the rate limits; the Durable Objects see it only hashed (the
  password lockout, reports, 2 public lobbies per IP).

## Self-hosting

[design: #3795 section 6, "Self-hosting", and goal G8]

`crates/nvf-signal-server` runs the stand-in (`crates/nvf-signal`, the same rules as the
Worker) as a server on one TCP port: `nvf-signal-server --listen 0.0.0.0:8787`. Players set
`NVF_COOP_SIGNAL=http://that-machine:8787`.
- It speaks plain HTTP and `ws://`, with no TLS. On the internet, put a TLS proxy in front.
- Codes and secrets come from the operating system's random source. The stand-in's seeded
  generator is for tests only: its outputs give its state away.
- It has no rate limit and keeps nothing on disk.
- It also serves section 12's offline smoke test: two games and the server on one PC, no
  internet.

## Example

```
host  → POST /v1/lobbies …                      ← {"code":"K7QM2X","host_secret":"s",…}
host  → {"type":"host","wire":1,"host_secret":"s"}
                                                ← {"type":"hosting","code":"K7QM2X","peers":[]}
guest → GET /v1/lobbies/k7q-m2x                 ← {"protocol":138,…,"full":false}
guest → {"type":"join","wire":1,"name":"Six","key":"0606…","candidates":[{"kind":"host","addr":"192.168.2.2:47960"}]}
guest ← {"type":"waiting","peer":1}             host ← {"type":"join","peer":1,"name":"Six",…}
host  → {"type":"accept","peer":1,"candidates":[{"kind":"srflx","addr":"203.0.113.5:47960"}]}
guest ← {"type":"accepted","host_key":"0101…","host_name":"Courier","candidates":[…]}
host  ← {"type":"go","peer":1}                  guest ← {"type":"go"}
guest → {"type":"candidates","candidates":[{"kind":"srflx","addr":"198.51.100.9:61000"}]}
                                                host ← {"type":"candidates","peer":1,"candidates":[…]}
guest → {"type":"connected","path":"direct"}    host ← {"type":"connected","peer":1,"path":"direct"}
```
