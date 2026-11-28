# Co-op: running your own relay or matchmaking

NVForever's co-op connects players' games straight to each other. Two small programs let you
run your own pieces of it, for yourself and your friends. The project runs neither for you.

| Program | What it does | When you want it |
|---|---|---|
| `nvf-relay` | Passes encrypted game packets between two players whose networks cannot reach each other | Your lobby fails with "Couldn't reach the host from your network", and no third player can relay |
| `nvf-signal-server` | Hands out lobby codes and introduces players, as `coop.nv-forever.com` does | You want no dependence on the project's service, or you play on a LAN with no internet |

Both are built from this repository (`cargo build --release -p nvf-relay -p nvf-signal-server`)
and need nothing else: no database, no files, no account.

## `nvf-relay`

Run it on a machine both players can reach: a cheap VPS, or a home server with a UDP port
forwarded.

```
nvf-relay --listen 0.0.0.0:47961
```

- Open UDP port 47961 on that machine's firewall, and forward it on its router if it is at
  home.
- Each player who wants to use it puts `Relay=relay.example.net:47961` under `[Coop]` in their
  settings. One player having it is enough: the other learns the address while connecting.
- The game uses the relay only when nothing direct answers within a couple of seconds.
- The relay sees only encrypted packets. It cannot read the game or change it. What it does
  learn is the players' IP addresses.
- `--max-sessions N` (default 256) and `--rate BYTES` (bytes a second per player, default
  262144) cap what it will carry. A co-op game uses about 15-20 KB/s each way.
- Every minute it prints one line: sessions, packets forwarded, dropped and refused. It keeps
  nothing on disk.

## `nvf-signal-server`

```
nvf-signal-server --listen 0.0.0.0:8787
```

- Every player sets `NVF_COOP_SIGNAL=http://that-machine:8787`, or `SignalUrl=` under
  `[Coop]`. Hosts and guests must use the same server: a code from one server means nothing
  to another.
- It speaks plain HTTP. On a LAN that is fine. On the internet, put a TLS proxy in front of
  it (for example Caddy or nginx with a certificate) and give players the `https://` address.
- It does what the Worker does: the same lobby codes, join approval, address exchange and peer
  relay introductions, with the rules in `docs/design/coop-signal-wire.md`. It has no rate
  limit, so do not leave it open to the whole internet without a proxy that limits requests.
- `--stun HOST:PORT` (repeatable) sets the STUN servers it hands to hosts. The default is
  Cloudflare's and Google's public ones. `--min-protocol N` turns away older game versions.
- Lobbies live in memory only and end when the host leaves.

## What neither changes

Game traffic always goes from player to player, or through a relay you chose. Every packet is
encrypted end to end between the host and each guest.
