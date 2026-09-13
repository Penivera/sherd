# sherd network

Cross-device Wi-Fi messaging service. One device hosts a hotspot while
staying connected upstream (STA+AP); other sherd devices join it. The
"just works" flow: run `sherd-daemon`, and it automatically joins a nearby
sherd network if one exists, or hosts its own if it doesn't.

Windows-first (see the plan at
`C:\Users\Lenovo\.claude\plans\windows-is-a-much-cheeky-mango.md` for the
full design rationale); every OS-specific piece sits behind the traits in
`sherd-platform`, so a real Linux backend can be dropped in later without
touching `sherd-core`, the daemon, the wire protocol, or any client.

## Layout

- `sherd-protocol` — wire types (`Request`/`Response`/`Event`) shared by the
  daemon and every client. No platform/storage deps.
- `sherd-platform` — OS-agnostic traits (`WifiCapabilityChecker`,
  `HotspotController`, `StationConnector`, `InterfaceEnumerator`).
- `sherd-platform-windows` — Windows backend: `netsh` for hotspot/station
  control, WlanAPI (via the `windows` crate) for interface enumeration.
- `sherd-platform-linux` — stub backend (not implemented yet); every method
  returns `NotImplemented`, with doc comments naming the real mechanism
  (`iw`, `hostapd`, `wpa_supplicant`, 802.11s/`batman-adv`) to use later.
- `sherd-core` — domain/service layer: SQLite-backed storage
  (contacts/conversations/messages, unused so far), and `SherdService`,
  which orchestrates the platform backend and implements the
  join-or-host `auto_connect()` flow.
- `sherd-daemon` — the background service binary. Owns `SherdService` and a
  local IPC socket (named pipe on Windows / Unix socket on Linux, via
  `interprocess`) that clients connect to.
- `sherd-cli` (binary name `sherd`) — reference IPC client; also the
  easiest way to drive the daemon by hand.

## Running it

```sh
cargo run -p sherd-daemon      # starts the service, attempts auto-connect immediately
cargo run -p sherd-cli -- auto        # same flow, explicitly
cargo run -p sherd-cli -- capability  # can this adapter host a network?
cargo run -p sherd-cli -- status      # capability + hotspot + station snapshot
cargo run -p sherd-cli -- hotspot start --ssid <ssid> --key <key>
cargo run -p sherd-cli -- hotspot stop
cargo run -p sherd-cli -- station connect --ssid <ssid> --key <key>
cargo run -p sherd-cli -- station disconnect
```

## Known limitations (by design, for this milestone)

- **Hosting requires Administrator.** `netsh wlan set/start hostednetwork`
  refuses without elevation; run `sherd-daemon` from an elevated prompt if
  this device is meant to host. Verified live: on a non-elevated prompt the
  daemon reports a clear command-failure error rather than crashing or
  misreporting it as unsupported hardware.
- **Shared passphrase is a placeholder.** `SherdConfig::shared_key` defaults
  to a single baked-in passphrase so any two sherd installs can find and
  join each other with zero setup — which is also its downside: as shipped,
  any sherd install can join any other's hotspot. Needs a real
  configured/paired secret before this leaves a trusted setting. See the
  doc comment on `sherd_core::config::SherdConfig`.
- **`netsh` output parsing is English-locale-shaped.** Ambiguous parses
  fail soft (treated as station-only / unknown) rather than panicking —
  see the unit tests in `sherd-platform-windows`.
- **Legacy `hostednetwork` doesn't route traffic by itself** — Internet
  Connection Sharing still needs configuring separately for upstream
  traffic to reach hosted-network clients. Not automated yet.
- **No message relay yet.** `SendMessage`/`SendFile` are already in the
  wire protocol and reach `SherdService`, but return "not yet implemented"
  — the next milestone wires a `libp2p` (mDNS + gossipsub) transport over
  whatever IP links this layer establishes.

## Verified so far

- `cargo build --workspace` and `cargo test --workspace` are clean.
- Live end-to-end: started the daemon, confirmed `sherd status`/`capability`
  round-trip over the real IPC socket, confirmed `auto` correctly reports
  `Unavailable` with a clear reason on this machine's actual
  station-only adapter (no sherd network was nearby to join either), and
  confirmed a hotspot-start attempt fails with a clear permissions error
  rather than a crash or a wrong diagnosis.
