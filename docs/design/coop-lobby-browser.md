# Co-op lobby browser: public sessions on coop.nv-forever.com

Status: design, 2026-11-24, at the owner's request; its four decisions taken 2026-11-25 (§10). Builds on docs/design/coop-networking.md
(#3795) and the matchmaking service's wire, docs/design/coop-signal-wire.md. Built 2026-11-25:
the service's wire 2 (the Worker, live) and the game's side (branch `claude/coop-net-p1`: the
host's Visibility, Title and Password; the Join screen's Code and Browse tabs; favourites in
`coop-favourites.txt`), checked offline through `nvf-signal-server` with screenshots at 1280x720,
1920x1080 and 1024x768. The lobby screens were redone in the bug report form's style at the
owner's word (bordered panel and boxes, 80% of the screen's width). Run live 2026-11-25
[observed] on two machines on one home network through coop.nv-forever.com (build 41737f5bb):
the desktop hosted a public lobby with a title and password; the laptop's Browse tab listed it
(title, place, 1/6, estimated ping, region, lock) and opened its row; a join with the password
came in (LAN path, 0 desyncs, 0 drops), as did a join by code with the password; a wrong
password and no password were refused with their messages before the host heard of them.

**On main** (2026-11-28, #3795, the port after #4901) [design]: the game's side was ported onto
main's Co-op screens (`engine/nvf-play/src/lobby.rs`, `engine/nvforever/src/coop/browse.rs`,
`coop/connect.rs`) in their simpler style: the host's Visibility, Title and Password boxes with
Show Password; the Join screen's Code tab (code, password) and Browse tab (Refresh, Hide empty,
Hide full, All versions, Search, the Name/Location/Players/Ping/Region headings, rows of
eight (`BROWSE_ROWS`) at a time; an opened row's Join with its password box, Favourite, Report).
Differences from §7 as built there: the estimated ping shows as `~N ms`, not bars; Visibility and
Title are not remembered between runs (the lobby's environment carries them across a load's
restart only); favourites are in `coop-favourites.txt` in the engine home. The two-machine run
above was the branch's; main's was checked offline only (the smoke scenario
`coop-online-local-duo-browse`, the unit tests of both files).

## 0. In one page

Today a lobby is private: a guest needs its code, and the service only answers "what is
behind this code" (`GET /v1/lobbies/{code}`) [documented: coop-signal-wire.md]. There is no
list of lobbies.

The browser adds one:

- **Host:** a host may make its lobby **public**, with a title, and may give any lobby, private
  or public, a **password**. The service lists public lobbies.
- **Guest:** the game's Join Lobby screen gets a **Browse** tab: a table of public lobbies with
  filters, search, favourites and Refresh.
- **Joining:** picking a row joins as a typed code does today. Join approval, Kick and Ban
  still apply.
- **The service:** a lobby's own Durable Object keeps the listing current, and one more Durable
  Object, the **Directory**, holds every public lobby's summary **in memory** for the list. No
  database, as now.
- **What the list never shows:** IP addresses or ports. The ping column is an estimate from
  Cloudflare's data-centre locations; the real ping shows in the lobby after joining.
- **Who runs it:** the same Worker, in the same repository (`NVForever-Coop`). No new
  Cloudflare app and no new repository: one new Durable Object class and its migration.

## 1. What a listing holds

The host's game sends its listing; the service adds the counts and the region it knows. Every
text is cut to its length and passed through the name filter (§6).

| Field | From | Shown as | Notes |
|---|---|---|---|
| `code` | service | (hidden) | What the game joins with. |
| `title` | host, ≤ 40 characters | Name | The host's own words. Default: "<host name>'s game". |
| `host_name` | host, ≤ 24 | (tooltip) | As in the lobby today. |
| `place` | host, ≤ 32 | Location | The host's current location as the game names it, refreshed when it changes. |
| `players`, `max_players` | service | Players | `players` counts the host, as `full` does now [documented: coop-signal-wire.md, Lifetimes]. |
| `locked` | host | Lock icon | A password is set (§4). |
| `friendly_fire` | host | (tooltip) | Off or on. |
| `protocol`, `build`, `load_order_hash` | host, as now | (filter) | Rows this game cannot join are hidden by default (§3). |
| `plugins` | host | (tooltip) | How many plugins are loaded, not their names. |
| `region` | service | Region | From Cloudflare's `request.cf` (continent, country) of the host's socket [hypothesis: Cloudflare's `IncomingRequestCfProperties`, to confirm against Sources]. |
| `colo` | service | Ping bars | The data centre that took the host's socket (`cf.colo`), for the estimate in §5. |
| `age` | service | (sort) | Seconds since the lobby was made. |

Left out on purpose: IP addresses, candidates, keys, the host's account or machine names, and
plugin names. A plugin list identifies players, and a load-order mismatch is already caught by
the hash.

## 2. Wire changes (version 2 of the service)

The service's `wire` goes from 1 to 2. A version-1 game keeps working and its lobbies stay
private: nothing changes for it.

**Create** (`POST /v1/lobbies`) takes an optional `listing`:

```json
"listing": {
  "public": true,
  "title": "Courier's Strip run",
  "place": "The Strip",
  "friendly_fire": false,
  "plugins": 14,
  "locked": true
}
```

`locked` only says a password will be set. The password's hash comes over the socket once the
code is known, since the code salts it (§4):

```json
{"type": "password", "hash": "<hex, §4>"}
```

An empty `hash` removes the password. Until the first `password` arrives, a `locked` lobby
declines every join (`bad_password`), so it is never open by accident.

**The host's socket** also takes a `listing` message. The service passes it to the Directory at
most once every 5 s:

```json
{"type": "listing", "place": "Goodsprings", "public": true, "title": "…", "friendly_fire": false}
```

- `public: false` takes the lobby off the list at once; `true` puts it back.
- The player count needs no message: the service already knows who is accepted.

**Browse:** `GET /v1/browse`. Its query:

| Parameter | Meaning |
|---|---|
| `protocol` | Only lobbies this game can join (the default in the game). |
| `load_order` | Only lobbies with this load-order hash; empty: any. |
| `q` | Search in title, host name and place (case-insensitive, ≤ 32 characters). |
| `empty`, `full` | `0` hides lobbies with only the host, or full ones. |
| `cursor` | The next page, from the last answer. |

It answers at most 100 rows a page:

```json
{"lobbies": [{"code": "K7QM2X", "title": "…", "host_name": "…", "place": "…",
  "players": 3, "max_players": 6, "locked": false, "friendly_fire": false,
  "plugins": 14, "region": "OC/AU", "colo": "SYD", "age": 812}],
 "cursor": null, "total": 41}
```

**Look-up** (`GET /v1/lobbies/{code}`) gains `locked`, so the game asks for the password before
joining a private lobby by code.

**Join** gains an optional `password` in `join`. The lobby's Durable Object checks it (§4).
A wrong one gets `declined` `bad_password`; three wrong in a minute from one IP get the 429 of
§6.

**Rate limits:** browse at most 1 request every 2 s per IP. The game's Refresh button waits
that long too. The zone rule of today (10 requests per 10 s on `/v1/lobbies`) gains
`/v1/browse` [documented: coop-signal-wire.md, Errors].

## 3. The Directory

- **What it is:** one Durable Object (`Directory`, one instance named `"main"`) holding a map of
  code → listing in memory.
- **Who feeds it:** a lobby's Durable Object sends it `put` when the lobby becomes public or its
  listing or count changes (at most every 5 s), and `drop` when it closes or turns private.
  Each `put` lives 90 s unless renewed; a lobby renews every 30 s even when nothing changed. A
  lost `drop` (a lobby DO evicted) therefore clears itself.
- **Answering browse:** the Worker asks the Directory, which filters and sorts, and caches the
  answer per query for 2 s (the Workers Cache API) [hypothesis: Cloudflare's Cache API, to confirm].
- **Scale:** 10,000 public lobbies at ~300 bytes each is ~3 MB, well inside a Durable Object's
  memory (128 MB) [hypothesis: Cloudflare's Durable Objects limits page, to confirm]. One instance takes
  ~1,000 requests a second [hypothesis: same page, soft limit, to confirm]. With the 2 s cache that is far
  more than the game needs.
- **When it outgrows one instance:** shard by region (one Directory per continent), with
  browse asking the player's own continent first. Not before it is needed [design].
- **Nothing stored:** a Directory that restarts empty fills again within 30 s from the lobbies'
  renewals. This keeps the service's promise of no database and no stored objects
  [documented: coop-signal-wire.md, What the service keeps].

## 4. Passwords and private lobbies

Visibility and the password are two separate choices:

- **Private** (the default): not listed. Joining by code works as today.
- **Public:** listed. Anyone who finds it may ask to join, and join approval still asks the host
  (on by default, owner decision 2026-11-24).
- **A password, on either:** the lobby asks every guest for it before join approval.
  - Private with a password: a guest needs the code and the password. The Code tab shows a
    password box once the look-up says `locked`.
  - Public with a password: listed with a lock, and the guest types the password in the row,
    as in the example screen.
- **How the password is checked:**
  - The host's game sends, in its `password` message, hex of SHA-256 over `lobby-password v1`,
    the lobby code and the password. The guest's game sends the same hash in `join`. The service compares the two and
    never sees the password itself.
  - The code is in the salt, so the same password hashes differently in each lobby [design].
  - A password is a gate against strangers, not secrecy: the content of the game is already
    protected by Noise between the games.

## 5. Ping without addresses

A real ping needs a packet between the two games, which would give every browser the host's
address. The browser estimates instead:

- **The data centre:** each row carries the Cloudflare data centre the host's socket went to
  (`colo`, an IATA airport code), and the browse answer carries the browsing player's own.
- **The estimate:** the game turns the two into a distance with a small table of data-centre
  coordinates (public airport locations; the codes come from the answer), and the distance
  into a round trip: about 1 ms per 100 km, plus 10 ms [design: light in fibre covers about
  200 km per ms one way; routes are longer than great circles].
- **Shown as:** signal bars (Near, Region, Far, Very far) with the estimate in the tooltip,
  marked "estimated".
- **The real ping:** shows on the host's lobby line per guest after joining (built:
  `Session::paths`).

## 6. Abuse

- **Text:** titles, host names and places go through a word filter on the service. The filter is
  NVForever's own list, kept in the Worker's repository. A filtered title shows as "<host
  name>'s game".
- **Listing limits:** at most 2 public lobbies per IP. Titles in all capitals are lowered.
- **Reports:** an opened row has Report, which asks "Report this lobby to the service?"
  (Report / Cancel) and then says in the corner whether it went through; a game reports a lobby
  once. The service keeps a count of reports per code in memory only; at 5 reports from 5 IPs
  the lobby is hidden from browse (still joinable by code). No report is stored after the lobby
  closes. Built 2026-11-25 (`lobby.rs` Report, `coop/browse.rs` `report`).
- **Ban lists stay the host's:** the host's Ban applies to keys; no account system.

## 7. What players see

**Host, in the Co-op Lobby (Create Lobby):**

- **Visibility:** Private / Public. A list row, Private first, and remembered.
- **Title:** a text field; empty: "<name>'s game".
- **Password:** a text field, empty for none, for either visibility. Typed characters are
  hidden, with a Show button. It is not remembered: each new lobby starts with none [design:
  a password left in NVForever.ini would sit in plain text].
- **Mods:** the code line stays; a public lobby also says "Listed in the server browser".

**Guest, Join Lobby:** two tabs, **Code** (today's screen, plus a password box when the code's
lobby is locked) and **Browse**.

- **Toolbar:** Refresh, Hide empty, Hide full, Show all versions (off: only lobbies this game can
  join), Search.
- **Columns:**

  | Column | What it shows |
  |---|---|
  | Favourite | The heart. |
  | Name | `title` |
  | Location | `place` |
  | Players | `players` / `max_players` |
  | Ping | Estimated bars (§5) |
  | Region | `region` |
  | Lock | The lock icon |

- **Sorting:** by any column; by Ping by default. The headings Name, Location, Players, Ping
  and Region are boxes: one sorts by it (A to Z and the nearest first; Players the fullest
  first), again turns it round; the sorted heading is filled and marked `^` or `v`. Favourites
  stay first whatever the sort. Built 2026-11-25.
- **Choosing a row:** the row opens to Join (and a password field when locked), as in the
  example. Join goes on to the save choice, as a code does today.
- **Rows this game cannot join:** greyed with the reason (another version, another load order)
  when Show all versions is on.
- **Favourites:** kept in `NVForever.ini` `[Coop] Favourites=` as the hosts' key fingerprints.
  A favourite host's new lobby is found again whatever its code. Favourites sort first.
- **Empty list:** "No public lobbies right now. Host one, or join a friend's by code."

**Settings:** none new. A host's choices belong to its lobby, kept in `[Coop]` (`Visibility=`,
`Title=`).

## 8. Privacy

- **What browsing gives a stranger:** a title, a name the host chose, a location in the game, a
  player count and a country-level region. No address.
- **When the host's address is known:** joining reveals the host's address to the guest, as a
  code does today. With join approval on, only after the host lets the guest in.
- **What the service keeps:** listings live only in memory and only while the lobby lives.
  Browse requests are not logged (invocation logs stay off).

## 9. Phases

| # | Work | Where |
|---|---|---|
| 1 | Wire 2: `listing`, `browse`, the Directory, password check, rate limits; the stand-in and conformance tests | NVForever-Coop (Worker), `crates/nvf-signal` |
| 2 | The host's Visibility, Title and Password; the `listing` message on place changes | `engine/nvf-coop/src/online.rs`, `engine/nvforever/src/coop.rs`, `nvf-play` lobby |
| 3 | The Browse tab: table, filters, search, favourites, password row | `engine/nvf-play/src/lobby.rs` |
| 4 | Ping estimate (data-centre table), reports, word filter | Worker; `nvf-signal` (table) |

Test: the stand-in server lists lobbies offline (`nvf-signal-server`). The smoke scenario
`coop-online-local-duo-browse` runs the service in the game (`NVF_COOP_SIGNAL=local`) and hosts
a public, locked lobby; a guest with no window (`NVF_COOP_TEST_GUEST`) joins it, and the Browse
tab lists it, sorts by Players and reports it, in one launch [design: the smoke runner starts
one game per scenario]. A two-machine run browses and joins a public lobby, then a locked one
(run 2026-11-25, see the status line).

## 10. Decisions (owner, 2026-11-25)

1. **Default visibility: Private.** Nothing changes unless the host chooses Public.
2. **Reports: on.** A lobby is hidden from browse at 5 reports from 5 IPs.
3. **Ping: estimated bars.** No addresses; the real ping only after joining.
4. **Word filter: NVForever's own short list**, kept in the Worker's repository.

## Sources

- Cloudflare Durable Objects limits: https://developers.cloudflare.com/durable-objects/platform/limits/
- Cloudflare `request.cf` properties: https://developers.cloudflare.com/workers/runtime-apis/request/#incomingrequestcfproperties
- Cloudflare Cache API in Workers: https://developers.cloudflare.com/workers/runtime-apis/cache/
- The service's wire today: docs/design/coop-signal-wire.md
