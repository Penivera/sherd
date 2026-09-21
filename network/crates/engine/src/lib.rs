//! Platform-agnostic domain and orchestration layer. Depends only on
//! `sherd-platform` (traits) and `sherd-protocol` (wire types shared with
//! clients) — never on a concrete OS backend. `sherd-daemon` wires a real
//! [`platform::PlatformBackend`] in and drives everything from here.

pub mod config;
pub mod entity;
pub mod models;
pub mod service;
pub mod storage;

pub use config::SherdConfig;
pub use service::{FeatureNotReady, SherdService};
pub use storage::Storage;
