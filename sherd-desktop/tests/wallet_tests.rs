use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sherd_desktop::auth::wallet::SolanaSigner;

#[test]
fn test_solana_signer_key_generation_and_verification() {
    let signer = SolanaSigner::new_ephemeral();
    let pubkey_b58 = signer.public_key_b58();

    // Verify it is a valid base58 32-byte public key
    let pubkey_bytes = bs58::decode(&pubkey_b58)
        .into_vec()
        .expect("Public key should decode as valid base58");
    assert_eq!(pubkey_bytes.len(), 32, "Ed25519 public key must be 32 bytes");

    let verifying_key = VerifyingKey::from_bytes(pubkey_bytes.as_slice().try_into().unwrap())
        .expect("Verifying key must be valid Ed25519 point");

    // Sign message
    let challenge = "Sign this message to authenticate with Sherd: challenge_xyz_12345";
    let signature_b58 = signer
        .sign_message(challenge)
        .expect("Signing message should succeed");

    let sig_bytes = bs58::decode(&signature_b58)
        .into_vec()
        .expect("Signature should decode as valid base58");
    assert_eq!(sig_bytes.len(), 64, "Ed25519 signature must be 64 bytes");

    let signature = Signature::from_bytes(sig_bytes.as_slice().try_into().unwrap());
    assert!(
        verifying_key
            .verify(challenge.as_bytes(), &signature)
            .is_ok(),
        "Signature verification should succeed"
    );

    // Tampered message should fail
    let tampered = "Tampered message";
    assert!(
        verifying_key
            .verify(tampered.as_bytes(), &signature)
            .is_err(),
        "Tampered message verification should fail"
    );
}
