use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("Failed to sign challenge message")]
    SigningFailed,
    #[error("Invalid public key")]
    InvalidKey,
}

pub struct SolanaSigner {
    signing_key: SigningKey,
}

impl SolanaSigner {
    pub fn new_ephemeral() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    pub fn public_key_b58(&self) -> String {
        let verifying_key = self.signing_key.verifying_key();
        bs58::encode(verifying_key.as_bytes()).into_string()
    }

    pub fn sign_message(&self, message: &str) -> Result<String, WalletError> {
        let signature = self.signing_key.sign(message.as_bytes());
        Ok(bs58::encode(signature.to_bytes()).into_string())
    }
}
