# Sherd

Sherd is a decentralized edge compute network.

## Architecture

The project is organized into modular services and native applications:

* **`sherd-desktop/`**: Native desktop client built in Rust using Zed's GPUI framework (`gpui 0.2.2`).
  - Native GPU-accelerated desktop UI (480x800).
  - Pure native Rust authentication layer (Argon2, embedded SQLite with `rusqlite`, Ed25519 Solana challenge verification, HS256 JWTs, loopback OAuth callbacks).
  - Zero Python / FastAPI / Uvicorn runtime dependencies.
  - Secure credential storage using OS keychain (`keyring`) with resilient in-memory session caching.
  - Native Ed25519 Solana signing (`ed25519-dalek`).
  - Interprocess daemon communication (`sherd-ipc.sock`).
* **`network/`**: Protocol, daemon, core, platform, and CLI network crates.

## Building and Running the Desktop Client

### Prerequisites
* Rust toolchain (1.80+)
* Linux system libraries: `libxkbcommon`, `libxkbcommon-x11`, Wayland/X11

### Running the Native Desktop Client
```bash
cargo run --manifest-path sherd-desktop/Cargo.toml
```

### Running Desktop Tests
```bash
cargo test --manifest-path sherd-desktop/Cargo.toml
```

All integration tests are located in separate test modules under `sherd-desktop/tests/`:
- `native_auth_db_tests`: SQLite embedded auth store, user creation, provider linking, replay attack protection.
- `native_email_auth_tests`: Argon2 password hashing and email authentication flows.
- `native_solana_auth_tests`: Server-side Solana challenge generation and Ed25519 verification.
- `native_auth_manager_tests`: Complete `AuthManager` coordinator and session lifecycle.
- `protocol_tests`: Daemon wire protocol message framing and codecs.
- `session_tests`: Session model serialization, expiration, and corruption handling.
- `wallet_tests`: Client-side Solana signer key generation and verification.