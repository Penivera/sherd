# Sherd

Sherd is a decentralized edge compute network.

## Architecture

The project is organized into modular services and native applications:

* **`desktop/`**: Native desktop client built in Rust using Zed's GPUI framework (`gpui 0.2.2`).
  - Native GPU-accelerated desktop UI (480x800).
  - Pure native Rust authentication layer (Argon2, embedded SQLite with `rusqlite`, Ed25519 Solana challenge verification, HS256 JWTs, loopback OAuth callbacks).
  - Zero Python / FastAPI / Uvicorn runtime dependencies.
  - Secure credential storage using OS keychain (`keyring`) with resilient in-memory session caching.
  - Native Ed25519 Solana signing (`ed25519-dalek`).
  - Interprocess daemon communication (`sherd-ipc.sock`).
* **`network/`**: Protocol, daemon, engine, platform, and CLI crates for joining/hosting the Wi-Fi hotspot devices use to reach each other.
* **`wire/`, `mesh/`, `mempool/`**: binary wire protocol, UDP gossip-mesh node (peer discovery, identity, dedup), and distributed task pool -- a separate, independent stack from `network/` today (nothing wires them together yet).
* **`vm-backend/`, `web-ui/`**: unrelated side project -- a small Node/Express backend and viewer page for streaming a third-party cloud VM.

## Using the network (`network/`)

This is the part of Sherd that lets nearby devices find each other over
Wi-Fi with zero setup, and send text messages/files between them. It's two
programs:

* **`daemon.exe`** (crate `network/crates/daemon`) — runs in the background,
  does all the actual work. This is the one you start and leave running.
* **`sherd.exe`** (crate `network/crates/cli`) — a command-line tool you run
  whenever you want to talk to the daemon (check status, send a message,
  etc.). It's quick, one command in, one answer out, and it exits — it's not
  something you leave open (except `sherd listen`, see below).

They talk to each other over a local connection on the same machine
(`sherd-ipc.sock`); `sherd.exe` never talks to the network directly.

### How it works

1. **Getting on the same network.** When the daemon starts, it looks for a
   nearby Wi-Fi network named `Sherd-...`. If it finds one, it joins it. If
   this device's Wi-Fi adapter is capable of it, it **also always turns on
   its own hotspot** (also named `Sherd-...`) at the same time, regardless
   of whether it found another network to join — every device that's
   capable of hosting does, so the network's range extends outward through
   every device rather than depending on just one. This is checked
   continuously (about every 15 seconds): if a hotspot or connection drops
   for any reason, it's automatically restarted/rejoined without you having
   to do anything.
2. **Getting an identity.** The very first time it runs, the daemon
   generates a permanent ID for this device (a cryptographic key, saved to
   `%APPDATA%\sherd\identity.key` on Windows) and picks a display name (your
   computer's name, by default). This ID stays the same every time you
   restart the daemon — it's how other devices recognize "this is the same
   device as before."
3. **Finding other devices.** Once on the same Wi-Fi network, every device
   announces itself a few times a minute ("I'm here, my name is ..., my ID
   is ..."). Every device keeps a live list of who's currently reachable.
4. **Sending things.** To send a text or a file to another device, you
   address it by its ID (or just the first few characters of it — see
   `sherd peers` below). The daemon opens a direct connection to that
   device and sends it.
5. **Receiving things.** The daemon starts listening for incoming
   messages/files the moment it starts — this needs nothing from you, and
   nobody has to be watching. Every message and file that arrives is saved
   straight away: text goes into a small local database
   (`%APPDATA%\sherd\sherd.db`, readable later with `sherd history`), and
   files get saved to `%APPDATA%\sherd\received\`. `sherd listen` (see the
   command table) is a completely separate, optional thing — it's just a
   window that prints messages the moment they arrive, for *watching* it
   happen live. Nothing is lost if that window isn't open; you just won't
   see it happen in real time, and would check `sherd history` instead.

### What has to stay running, and what doesn't

This trips people up, so to be explicit:

* **`daemon.exe` has to keep running, full stop.** It's the thing doing the
  hosting, joining, listening, and saving — if you close its window,
  *all* of that stops: no hotspot, no receiving, nothing, until you start
  it again. You can minimize its window; you just can't close it. There's
  no background-service or system-tray version yet (a natural next step,
  not built this pass) — for now, "running the daemon" means an open
  (if minimized) console window, or a terminal tab left alone.
* **`sherd.exe` (the CLI) does *not* need to stay open**, with one
  exception. `sherd status`, `sherd send`, `sherd peers`, etc. each run,
  print their answer, and exit immediately — there's nothing to leave
  running. The one exception is `sherd listen`, and even that's optional:
  it's only for watching messages arrive live in a terminal. Skipping it
  costs you nothing but real-time notification — every message/file is
  already saved by the daemon regardless, and `sherd history <id>` shows
  you everything you missed.

### Prerequisites

* Rust toolchain (1.80+) — only needed if you're building from source.
* **Run the daemon as Administrator on Windows.** Hosting a Wi-Fi hotspot
  needs elevated permissions; without it, hosting will fail (joining an
  existing network still works fine unelevated).
* The first time the daemon receives a connection from another device,
  **Windows Firewall will likely prompt you to allow it** on private/public
  networks — click Allow, or messaging/discovery from other devices won't
  reach it.

### Building it

```bash
cargo build --release -p daemon -p cli
```

This produces `target/release/daemon.exe` and `target/release/sherd.exe`
(the CLI is named `sherd`, not `cli`, so it doesn't collide with anything).

### Copying it to another PC

Both `.exe` files are self-contained — no installer, nothing else from the
`target` folder needs to travel with them. Copy both (USB stick, network
share, however) to the other machine and you're set, with three caveats:

* It needs to run **as Administrator** there too (see Prerequisites above).
* **Windows Firewall will likely prompt** the first time it talks to
  another device — click Allow, on both private and public networks.
* It's a 64-bit Windows build — fine for essentially any modern Windows PC,
  but it won't run on ARM or 32-bit Windows. If it fails to launch at all
  with a missing-DLL error, that PC is missing the Microsoft Visual C++
  Redistributable (x64) — usually already present, occasionally not on a
  bare-bones install.

### Running it

Start the daemon first (as Administrator — right-click, "Run as
administrator"), and **leave its window open** (minimized is fine) — see
"What has to stay running, and what doesn't" above for why:

```bash
daemon.exe
```

As soon as it starts, it's already joining/hosting and listening for
incoming messages on its own — there's nothing further to run just to
"turn on" receiving.

Then, whenever you want to check on it or do something, open a *separate*
terminal (or just double-click, for the no-argument default) and use the
CLI — each of these runs, prints its answer, and exits, without needing the
daemon's own window touched:

```bash
sherd            # same as `sherd auto`: join a nearby network, or host one
sherd status      # what's this device doing right now
```

### Command reference

| Command | What it does |
|---|---|
| `sherd` / `sherd auto` | Join a nearby sherd network, or host one if none is found. Runs automatically on daemon startup, too. |
| `sherd status` | Capability + hotspot + station link state. |
| `sherd capability` | Just the Wi-Fi capability check (can this device host?). |
| `sherd hotspot start --ssid <name> --key <pass>` / `sherd hotspot stop` | Manually control this device's own hotspot. |
| `sherd station connect --ssid <name> --key <pass>` / `sherd station disconnect` | Manually join/leave a network as a client. |
| `sherd whoami` | This device's permanent ID and display name. |
| `sherd peers` | Other sherd devices reachable right now. |
| `sherd send <id> <text...>` | Send a text message. `<id>` can be the short prefix `peers` shows. |
| `sherd send-file <id> <path>` | Send a file. |
| `sherd history <id>` | Past messages/files exchanged with a device (works even if it's offline right now, and even if `sherd listen` was never running when they arrived). |
| `sherd listen` | *Optional.* Leave this running only if you want to watch messages/files appear live in a terminal. Not needed for them to actually be received and saved — the daemon does that regardless. Ctrl+C to stop. |

### Trying it between two devices

1. Copy `daemon.exe` and `sherd.exe` to both machines (see "Copying it to
   another PC" above).
2. Run `daemon.exe` as Administrator on both. Give it a few seconds.
3. On either machine: `sherd status` — you should see a hotspot come up
   (`Hotspot: Up`) on whichever device(s) are capable, and the other
   device's `sherd status` should show `Station: Up` if it joined instead.
4. `sherd peers` on either machine should list the other once they're both
   on the same Wi-Fi network and a few seconds have passed for the
   discovery broadcast.
5. `sherd send <the-other-device's-short-id> hello!` — it arrives instantly
   on the other machine whether or not anyone's watching there. Check it
   with `sherd history <your-short-id>`, or start `sherd listen` on that
   machine *beforehand* if you want to watch it show up live.

### Known limitations

* **No background-service/tray version yet** — `daemon.exe`'s window has to
  stay open (minimized is fine) on both machines for any of this to keep
  working. Closing it stops hosting, joining, and receiving until it's
  started again.
* You can only message a device that's currently reachable (shown in
  `sherd peers`) — there's no "deliver later" queue yet.
* Files are sent whole, in memory — fine for everyday files, not built for
  very large transfers yet.
* Every Sherd install currently shares one built-in Wi-Fi password, so any
  device running this software can join any other's hotspot — there's no
  pairing/invite step yet. Don't rely on this for anything sensitive.

## Building and Running the Desktop Client

### Prerequisites
* Rust toolchain (1.80+)
* Linux system libraries: `libxkbcommon`, `libxkbcommon-x11`, Wayland/X11

### Running the Native Desktop Client
```bash
cargo run --manifest-path desktop/Cargo.toml
```

### Running Desktop Tests
```bash
cargo test --manifest-path desktop/Cargo.toml
```

All integration tests are located in separate test modules under `desktop/tests/`:
- `native_auth_db_tests`: SQLite embedded auth store, user creation, provider linking, replay attack protection.
- `native_email_auth_tests`: Argon2 password hashing and email authentication flows.
- `native_solana_auth_tests`: Server-side Solana challenge generation and Ed25519 verification.
- `native_auth_manager_tests`: Complete `AuthManager` coordinator and session lifecycle.
- `protocol_tests`: Daemon wire protocol message framing and codecs.
- `session_tests`: Session model serialization, expiration, and corruption handling.
- `wallet_tests`: Client-side Solana signer key generation and verification.