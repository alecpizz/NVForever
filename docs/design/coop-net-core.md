# Co-op: the network core

Status: built (`engine/nvf-coop`, `engine/nvf-play/src/lobby.rs`, `engine/nvforever/src/coop.rs`).
It is the first part of co-op, and the layer #3910 builds on: #3910 (the internet transport, Noise,
online codes, relays, join approval and Kick/Ban) goes on top of this part once it is merged. The
protocol here stays; only the transport underneath is replaced by #3910's, through the transport
seam below.

Tags as the charter asks. Everything here is `[design]`: co-op is NVForever's own, and the
original game has nothing to compare it with.

## What it does

- A host opens a lobby and up to five guests join it (`net::MAX_GUESTS`).
- Each game plays its own world, and each player's state reaches the others 20 times a
  second (`net::SEND_HZ`).
- A reliable, ordered stream (`rel.rs`) and the generic records it carries (`repl.rs`) are
  in place for the parts that share the world. Nothing uses them yet.
- What it does not do yet: draw the other players or share anything of the world. Those are
  the next parts, and they build on the hooks below.

## Off by default

- The Co-op switch (`nvf_play::lobby::CoopSwitch`) is a row of the settings page, `coop`
  (Gameplay, Co-op), so it is kept in the settings file; it is off in a new home.
- While it is off:
  - the main menu hides Join Lobby (it takes no room in the column);
  - the pause menu hides its Co-op entry;
  - no socket is opened.
- The command-line flags that start a lobby come with the internet transport on top.

## The seam, and the interim secret

- No real transport is in this part: no sockets, no discovery, no lobby codes. A game
  hosting or joining says so on its screen; the internet transport that goes on top (#3910)
  plugs into the seam below. The tests carry packets in memory, or over loopback UDP
  (`seam::loopback`, test-only).
- `seam::Transport` carries datagrams: `send_to`, `recv_from` and `local_addr`.
- `seam::Admission` decides who is let in; `SharedSecret` is its interim implementation:
  - a 64-bit secret per lobby, from the operating system's random source (`getrandom`);
  - the host drops every Hello whose proof is wrong or 0;
  - the host's Welcome (and Full) carries a value keyed with the secret, the guest's token
    and its slot, and a guest takes no Welcome that does not check out;
  - a token rejoin is honoured only after the secret check.
- **This is an interim check until the transport is encrypted** (#3910's Noise handshake):
  the secret travels in the clear in the Hello, and the Welcome's check value is not
  cryptography. Both sit behind `seam::Admission`, which the handshake replaces.
- `Session::host_on` and `Session::join_on` take both a transport and an admission.

## The host checks every guest message (`guard.rs`)

All of it is sans-IO and tested on its own:

- **Who sent it:** the sender is the peer at the packet's address, never an id in the
  message.
- **Host-only messages:** Welcome, Full, Who and Names from a guest are dropped.
- **Rates:** each guest has a token bucket per message kind (`guard::default_rate`, changed
  with `set_rate`). An empty bucket drops the message.
- **States:**
  - Every number must be finite.
  - Feet must be within the world's bounds, at most `net::MAX_WORN` items may be worn, and
    the place must be one the host knows.
  - In the same place, a guest moves no faster than `guard::MAX_SPEED`. A jump is taken only
    after the guest keeps reporting from the new spot for `guard::SETTLE`, so a fast travel
    within one worldspace still goes through.
  - The weapon must be a WEAP, each worn item an ARMO, and the furniture a placed FURN
    reference, by the host's load order (`guard::FormCheck`, which `coop.rs` implements).
  - A form that fails is cleared, and the rest of the state is kept.
- **Names, looks and reliable frames:** names are cleaned, and looks and reliable frames are
  bounded.
- **The admit hook:**
  - `Guard::admit(from_id, kind, subject)` runs after the built-in checks and before any
    event or forward.
  - Later parts register a `Rule` per message kind, and per reliable item by its first byte
    (`Kind::Item`).
  - Rules see the guest's last accepted state (`Guard::last_state`).
- **Counts:**
  - Drops are counted per kind and reason (`Session::guard_stats`).
  - Each guest has a bad-message count, which a later kick policy will read
    (`Session::bad_messages`).

## Bounds

- `heard` and `sent`, the per-address traffic counts, keep at most `net::TRAFFIC_ADDRS`
  addresses.
- The reliable resend queue holds at most `rel::MAX_UNACKED` items. A full queue refuses
  (`send_reliable` returns false).
- Events waiting for the game are capped at `net::EVENTS_MAX`:
  - a newer state or look replaces the waiting one;
  - reliable frames are not taken while the events are full;
  - the overflow is counted.
- Rate buckets are capped at `guard::BUCKETS_MAX`.
- Every list on the wire has a decode bound.

## Writes

- Co-op writes through `fnv_install::guard_write` first, in the home (`fnv_install::EngineHome`):
  the switch is kept by the settings page with the other rows, and a later part writes the
  lobby's code for the host to copy.
