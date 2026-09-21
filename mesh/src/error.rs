use std::io;
use std::net::SocketAddr;
use wire::{NodeId, WireError};
use thiserror::Error;
use crate::identity::IdentityError;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Wire error: {0}")]
    Wire(#[from] WireError),
    #[error("Identity error: {0}")]
    Identity(#[from] IdentityError),
    #[error("Peer not found in peer table: {0}")]
    PeerNotFound(NodeId),
    #[error("Peer address unreachable: {0}")]
    PeerUnreachable(SocketAddr),
    #[error("Handshake rejected: {0}")]
    HandshakeRejected(String),
    #[error("Node has been stopped")]
    NodeStopped,
}
