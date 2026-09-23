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

1. **Getting an identity.** The very first time it runs, the daemon
   generates a permanent ID for this device (a cryptographic key, saved to
   `%APPDATA%\sherd\identity.key` on Windows) and picks a display name (your
   computer's name, by default). This ID stays the same every time you
   restart the daemon — it's how other devices recognize "this is the same
   device as before." The device's hotspot name comes from it too
   (`Sherd-` plus the first 6 characters of the ID, e.g. `Sherd-1B13A2`), so
   the hotspot keeps the same name across restarts.
2. **Hosting a hotspot — always, if the device can.** If this device's
   Wi-Fi can host a hotspot, the daemon **always** turns it on, under its
   own name and Sherd's password. Every device that can host does, which is
   what lets the network's range reach further with each device instead of
   depending on just one.
3. **The daemon is in charge of the Wi-Fi.** Whenever a `Sherd-...` network
   is in range, the Wi-Fi stays connected to the nearest one (strongest
   signal; if joining one fails, the next is tried). A device connected to
   someone else's hotspot while running its own acts as a *repeater*,
   extending that network further. If anyone connects the Wi-Fi to a
   different network by hand, disconnects it, or turns Wi-Fi off, the
   daemon puts it back at its next check. Otherwise the device would
   silently drop out of the mesh and become unreachable. Two things to know:
   * **This includes your home Wi-Fi.** While a Sherd network is in range,
     the PC is moved off whatever network it was on. **To use the Wi-Fi
     normally, close the daemon.** Only when no Sherd network is in range is
     the Wi-Fi left as it is.
   * **It only switches for a clearly better signal.** It moves to a
     different Sherd network only if that one is much stronger (30+ points
     of signal). Every switch briefly cuts off anything connected through
     this device, so it doesn't hop back and forth between networks of
     similar strength.
4. **No loops.** Devices tell each other which hotspot they run and which
   network they're connected to, so if PC B is connected to PC A's hotspot,
   PC A never connects "back" into PC B's. That would be a circle with no
   route anywhere else. And when two devices first come into range, both
   would try to join the other at the same moment. So the one with the
   higher ID joins first, and the other waits 45 seconds, by which time
   it knows the first is connected through it.
5. **Staying on — automatically.** The daemon checks about every 15
   seconds, and puts right anything that's wrong:
   * The hotspot was switched off (e.g. in Windows Settings): turned back on.
   * The hotspot was **renamed, or its password changed**: changed back
     (other devices look for its Sherd name and use Sherd's password, so
     either change would cut them off). This briefly restarts the hotspot.
   * Wi-Fi was turned off, disconnected, or moved to another network:
     turned back on and reconnected, as above.

   If something keeps failing (say, Windows refuses to host), the daemon
   waits longer between tries — up to 5 minutes — and logs the problem once
   rather than every 15 seconds.
6. **Your internet is shared — you'll be told.** Windows' hotspot works by
   sharing an existing internet connection. If that's your own (home Wi-Fi,
   Ethernet), the daemon prints a clear warning the first time, and `sherd
   status` shows it too, because anyone nearby running Sherd can use it
   (every Sherd install currently uses the same built-in password). Close
   the daemon to stop sharing.
7. **Closing the daemon turns the hotspot off** (Windows takes up to about
   20 seconds to finish), and puts your own Mobile Hotspot name and
   password back the way they were. (Versions from before this change
   didn't do that. If Settings > Mobile hotspot still shows a `Sherd-...`
   name, rename it there once, with the daemon closed.)
8. **Finding other devices.** Every device announces itself every few
   seconds ("I'm here, my name is ..., my ID is ...") on *every* network
   it's on — including its own hotspot — and answers newcomers directly.
   Every device keeps a live list of who's currently reachable, and the
   daemon logs when a device appears or disappears.
9. **Sending things.** To send a text or a file to another device, you
   address it by its ID (or just the first few characters of it — see
   `sherd peers` below). The daemon opens a direct connection to that
   device and sends it.
10. **Receiving things.** The daemon starts listening for incoming
   messages/files the moment it starts — this needs nothing from you, and
   nobody has to be watching. Every message and file that arrives is saved
   straight away: text goes into a small local database
   (`%APPDATA%\sherd\sherd.db`, readable later with `sherd history`), and
   files get saved to `%APPDATA%\sherd\received\`. `sherd listen` (see the
   command table) is a completely separate, optional thing — it's just a
   window that prints messages the moment they arrive, for *watching* it
   happen live. Nothing is lost if that window isn't open; you just won't
   see it happen in real time, and would check `sherd history` instead.
   Every received message and file is also written to the daemon's own
   window, as it arrives.

### What has to stay running, and what doesn't

This trips people up, so to be explicit:

* **`daemon.exe` has to keep running, full stop.** It's the thing doing the
  hosting, joining, listening, and saving. If you close its window (or
  press Ctrl+C in it), it turns the hotspot off and stops, and *all* of
  that stops with it: no hotspot, no receiving, nothing, until you start it
  again. You can minimize its window; you just can't close it. There's no
  background-service or system-tray version yet (a natural next step, not
  built yet). For now, "running the daemon" means an open (if minimized)
  console window. (If it's killed some other way, like End task in Task
  Manager, it can't clean up, so the hotspot stays on until you turn it off
  in Settings.)
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
* **Run the daemon as Administrator on Windows.** Turning the hotspot on
  and switching Wi-Fi networks can fail without it. The daemon checks, and
  says so at startup if it isn't elevated.
* The first time the daemon receives a connection from another device,
  **Windows Firewall will likely prompt you to allow it** on private/public
  networks — click Allow, or messaging/discovery from other devices won't
  reach it.

### Building it

```bash
cargo build --release -p daemon -p cli
```

Run it from the repository's root folder. This produces
`target/release/daemon.exe` and `target/release/sherd.exe` (the CLI is named
`sherd`, not `cli`, so it doesn't collide with anything).

> **Old builds:** there may be an old `network/target/release/` folder with
> a `sherd-daemon.exe` in it. That's from before the project was
> reorganized and is out of date. Don't run or copy anything from there.
> The current programs are `daemon.exe` and `sherd.exe` in
> `target/release/`, at the root.

### Updating to a new version

Close the old daemon's window first (on every PC), then start the new
`daemon.exe`. Only one copy can run at a time. If an old one is still
running, the new one says so and exits. If `sherd` says the daemon "is an
older version that doesn't know this command", an old daemon is still
running.

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
"turn on" receiving. Its window shows, in plain sentences, what it's doing,
for example:

```
21:05:12  INFO Sherd is running on this device as "HIS-THINKPAD" (ID 1b13a2c4).
21:05:12  INFO Its hotspot is called "Sherd-1B13A2". Keep this window open (minimizing is fine) -- closing it turns the hotspot off and stops Sherd.
21:05:12  INFO Connecting to the mesh...
21:05:15  INFO Hotspot "Sherd-1B13A2" is on.
21:05:15  WARN Heads up: this hotspot is sharing this PC's internet connection ("MyHomeWiFi") with every device that joins the mesh ...
21:05:40  INFO OFFICE-PC (9f0c2e71) is now reachable.
21:06:02  INFO Message from OFFICE-PC (9f0c2e71): hello!
21:07:30  WARN The hotspot was renamed to "My Hotspot" -- fixing it.
21:07:41  INFO Hotspot "Sherd-1B13A2" is on.
21:09:02  WARN Wi-Fi is connected to "MyHomeWiFi" instead of a Sherd network -- fixing it.
21:09:08  INFO Joined the Sherd network "Sherd-9F0C2E".
```

(For more detail when troubleshooting, set `RUST_LOG=debug` before starting
it.)

Then, whenever you want to check on it or do something, open a *separate*
terminal and use the CLI. Each command runs, prints its answer, and exits,
without needing the daemon's own window touched. Double-clicking
`sherd.exe` shows the status and keeps its window open until you press
Enter:

```bash
sherd            # same as `sherd status`
sherd status     # what this device is doing right now
```

### Command reference

| Command | What it does |
|---|---|
| `sherd` / `sherd status` | What this device is doing: its name/ID, hotspot on/off, which Wi-Fi it's on, whether it's sharing your internet, and how many devices are in reach. |
| `sherd auto` | Re-run "join / turn the hotspot on" right now, instead of waiting for the daemon's next check. Rarely needed. |
| `sherd capability` | Can this device's Wi-Fi host a hotspot, or only join networks? |
| `sherd hotspot start --ssid <name> --key <pass>` / `sherd hotspot stop` | Manually control this device's own hotspot. (The daemon turns it back on at its next check. To keep it off, close the daemon.) |
| `sherd station connect --ssid <name> --key <pass>` / `sherd station disconnect` | Manually join/leave a network as a client. |
| `sherd whoami` | This device's permanent ID and display name. |
| `sherd peers` | Other sherd devices reachable right now. |
| `sherd send <id> <text...>` | Send a text message. `<id>` can be the short prefix `peers` shows. |
| `sherd send-file <id> <path>` | Send a file. |
| `sherd history <id>` | Past messages/files exchanged with a device (works even if it's offline right now, and even if `sherd listen` was never running when they arrived). |
| `sherd listen` | *Optional.* Leave this running only if you want to watch messages/files (and devices coming and going) live in a terminal. Not needed for them to actually be received and saved — the daemon does that regardless, and shows them in its own window too. Ctrl+C to stop. |

### Trying it between two devices

1. Copy `daemon.exe` and `sherd.exe` to both machines (see "Copying it to
   another PC" above).
2. Run `daemon.exe` as Administrator on both. Give it a few seconds.
3. On either machine: `sherd status`. You should see `Hotspot: on` on
   whichever device(s) can host. Within about a minute, one of the two
   also shows `Wi-Fi: connected to the Sherd network "Sherd-..."`: it has
   joined the other's hotspot. It's the one with the higher ID; the other
   waits so they don't join each other at once. The one that joined leaves
   whatever Wi-Fi it was on before.
4. `sherd peers` on either machine should list the other within a few
   seconds of them being on the same network. Each daemon's window also
   logs "... is now reachable".
5. `sherd send <the-other-device's-short-id> hello!` — it arrives instantly
   on the other machine whether or not anyone's watching there. Check it
   with `sherd history <your-short-id>`, or start `sherd listen` on that
   machine *beforehand* if you want to watch it show up live.

### Known limitations

* **No background-service/tray version yet** — `daemon.exe`'s window has to
  stay open (minimized is fine) on both machines for any of this to keep
  working. Closing it turns the hotspot off and stops joining and
  receiving until it's started again.
* Loop prevention covers direct loops (A connected to B's hotspot while B
  is connected to A's). Longer circles through three or more devices
  aren't detected yet.
* **Messages only reach devices one hop away.** Each hotspot is its own
  small network. A device reaches the hotspot it's connected to, and
  devices connected to its own hotspot, but not devices two or more
  hotspots away. Nothing forwards messages onward yet.
* If you close the daemon in the few seconds while it's restarting the
  hotspot (e.g. just after undoing a rename), Windows may not finish
  turning the hotspot off before the window disappears. Turn it off in
  Settings if so.
* The Wi-Fi control, the Wi-Fi-radio switch-on, and joining the nearest
  network are tested in isolation, and the hotspot-rename fix and
  shutdown are tested live on one PC. None of them have been tested
  between two real PCs yet.
* Some PCs report they can host, then Windows refuses when asked (seen as
  `Unspecified error (0x80004005)`). The daemon now explains what to check.
  If Mobile hotspot won't turn on by hand in Windows Settings either, that
  PC can't host, but it still joins other devices' hotspots.
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