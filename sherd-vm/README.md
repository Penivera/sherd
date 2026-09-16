# sherd-vm

Standalone Rust workspace for Sherd VM integration — Solari Linux desktops + pluggable Windows provider, file injection, VNC streaming, and IPC bridge to the desktop client.

**Outside `network/` workspace** — isolated heavy deps (`reqwest`, `tokio-tungstenite`, `uuid`) from mesh build. Talks to `sherd-daemon` via `sherd-ipc.sock` NDJSON and to Solari via `https://api.getsolari.com` (`slr_live_` bearer).

## Layout

```
sherd-vm/
  Cargo.toml                          # [workspace] members = [solari-desktop-rs, sherd-vm-core]
  crates/solari-desktop-rs/           # Separate Rust desktop client (buildable alone)
    src/client.rs                     # DesktopClient: create/get/destroy/health/pause/resume/exec/fs/screenshot/stream/mouse/keyboard
    src/error.rs                      # SolariError with retryable/concurrency helpers
    src/types.rs                      # CreateDesktopOpts, DesktopSession, Health, etc.
  crates/sherd-vm-core/
    src/config.rs                     # SolariConfig + SherdVmConfig (env: SOLARI_API_KEY, SHERD_AUTH_URL)
    src/error.rs                      # VmError
    src/providers/mod.rs              # trait VmProvider + CreateVmOpts, VmSession, VmStatus
    src/providers/solari_linux.rs     # SolariLinuxProvider (wraps solari-desktop-rs)
    src/providers/windows.rs          # WindowsProvider stub (NotImplemented, pluggable)
    src/vm.rs                         # VmManager: provision + health poll + setup_environment
    src/files.rs                      # FileLoader: upload_file/dir, ensure_wine, launch_windows_exe
    src/stream.rs                     # StreamBridge: get_stream_url, send_input, capture
    src/service.rs                    # VmService: broadcast VmEvent, status aggregation
    src/ipc.rs                        # IPC over sherd-vm.sock (VmRequest/VmResponse/VmServerMessage)
    src/main.rs                       # binary sherd-vm (clap)
```

## Providers

- **Linux** (`OsKind::Linux`): `SolariLinuxProvider` via Solari Desktop API. Templates: `default`/`workstation` (Ubuntu), `office` (+LibreOffice/GIMP), `code` (+VS Code). Custom via `TemplateClient` (not yet wired — use `fs.write` post-boot).
- **Windows** (`OsKind::Windows`): `WindowsProvider` stub returning `NotImplemented` (mirrors `sherd-platform-linux`). Replace with Hyper-V / Windows Sandbox / cloud Windows API (Azure/Paperspace) when available. For `.exe` demo on Linux VM, use Wine: `ensure_wine` + `wine64 /tmp/app.exe` + snapshot `wine-ready` → `fromSnapshot` for fast reuse.

Selection: `CreateVmOpts.os` + `SOLARI_PROVIDER` env. `VmManager` holds `Arc<dyn VmProvider>` per OS (like `sherd-platform` trait objects).

## Solari Wire

- Base: `https://api.getsolari.com`, auth `Authorization: Bearer slr_live_...`, `Idempotency-Key: uuid-v4` on creates (replayed 24h, `Idempotent-Replayed: true`).
- Errors: JSON `{error, code, retryable, plan, cap}` — branch on status/`code`, not prose. 429 `ConcurrencyLimitExceeded` not retryable; 502/503/504 `retryable:true` with exponential backoff 150ms→8s + jitter, max 5 retries.
- `sessionId` contains `:`/`.` — URL-encoded in paths. `DELETE` idempotent (second delete 200). `exec` 200 even if `exitCode !=0`.
- Idle: `timeoutMs` default 15m, `lifecycle: {onTimeout: "pause"}` (resumable, frees concurrency slot) vs `"kill"`. Paused VMs don't count against cap.

## Running

```sh
cp .env.example .env  # set SOLARI_API_KEY
cargo build -p sherd-vm-core
cargo run -p sherd-vm-core -- --help
cargo run -p sherd-vm-core -- create --os linux --template office --cpu 2 --mem 4096
cargo run -p sherd-vm-core -- status <session_id>
cargo run -p sherd-vm-core -- upload <session_id> ./app.exe /tmp/app.exe
cargo run -p sherd-vm-core -- exec <session_id> wine64 -- /tmp/app.exe
cargo run -p sherd-vm-core -- stream <session_id>
cargo run -p sherd-vm-core -- screenshot <session_id> --out /tmp/screen.png
cargo run -p sherd-vm-core -- destroy <session_id>

# IPC daemon for desktop client
cargo run -p sherd-vm-core -- serve
cargo run -p sherd-vm-core -- serve --socket sherd-vm.sock
```

## Desktop Client Integration

- `sherd-desktop-client/src/services/vmService.js` — thin client mirroring `meshService.js` pattern: `createVm`, `getVmStatus`, `getStreamUrl`, `uploadFile`, `sendInput`, `captureScreenshot`, `execCommand`, `listVms`. Tries IPC (`window.sherd.vmRequest` via `electron/preload.cjs`) → HTTP (`VITE_VM_BASE_URL`) → mock fallback.
- `electron/preload.cjs` exposes `window.sherd.vmRequest`; `electron/main.cjs` forwards to `http://localhost:8765/ipc` (or socket).
- Auth: `vmService.js` attaches `Authorization: Bearer` from `session.js` (JWT from `sherd-auth`). `sherd-vm` validates via `GET /auth/me` (`SHERD_AUTH_URL`).

## Testing

```sh
cargo check --workspace
cargo test --workspace
# Live integration (requires SOLARI_API_KEY):
cargo test -p sherd-vm-core -- --ignored --test-threads=1
```

Live test: create `default` VM → `health.ready` → `fs.write`/`readText` → `exec echo` → `screenshot` → `destroy` (second delete also 200).

## Further Considerations (Resolved)

1. **Linux + Windows**: trait `VmProvider` with `SolariLinuxProvider` + `WindowsProvider` stub; Wine snapshot for `.exe` on Linux.
2. **Missing Rust desktop crate**: built separately as `crates/solari-desktop-rs` (independent of `solari-sandbox = "0.1"`), raw HTTP/WebSocket, `trait DesktopController` abstraction for future swap.
3. **Concurrency cap** (free: 1 sandbox, 3 browsers): `onTimeout:"pause"` + immediate `destroy`, serialize tests, on 429 pause oldest idle before retry.
