use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
pub use wire::NodeId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("Invalid public key bytes: {0}")]
    InvalidPublicKey(String),
    #[error("Invalid signature bytes: {0}")]
    InvalidSignature(String),
    #[error("Signature verification failed")]
    VerificationFailed,
}

/// Cryptographic identity for a SHERD node, wrapping an Ed25519 signing key.
pub struct Keypair {
    signing_key: SigningKey,
    node_id: NodeId,
}

impl Keypair {
    /// Generate a fresh random Ed25519 keypair using OS entropy.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let node_id = NodeId::new(*verifying_key.as_bytes());
        Self {
            signing_key,
            node_id,
        }
    }

    /// Construct a keypair from raw 32-byte private seed.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(bytes);
        let verifying_key = signing_key.verifying_key();
        let node_id = NodeId::new(*verifying_key.as_bytes());
        Self {
            signing_key,
            node_id,
        }
    }

    /// Return the public NodeId for this keypair.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Sign arbitrary bytes with this keypair.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }
}

/// Verify an Ed25519 signature against a public NodeId.
pub fn verify_signature(
    node_id: &NodeId,
    message: &[u8],
    signature_bytes: &[u8; 64],
) -> Result<(), IdentityError> {
    let verifying_key = VerifyingKey::from_bytes(node_id.as_bytes())
        .map_err(|e| IdentityError::InvalidPublicKey(e.to_string()))?;
    let signature = Signature::from_bytes(signature_bytes);
    verifying_key
        .verify(message, &signature)
        .map_err(|_| IdentityError::VerificationFailed)
}
