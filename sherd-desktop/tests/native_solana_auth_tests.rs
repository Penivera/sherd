use sherd_desktop::auth::db::AuthDb;
use sherd_desktop::auth::solana::{SolanaAuthError, SolanaAuthService};
use sherd_desktop::auth::token::TokenManager;
use sherd_desktop::auth::wallet::SolanaSigner;

#[test]
fn test_solana_challenge_creation_and_successful_verification() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");
    let service = SolanaAuthService::new(db, TokenManager::default());

    let signer = SolanaSigner::new_ephemeral();
    let wallet_address = signer.public_key_b58();

    // 1. Issue challenge
    let challenge = service
        .create_challenge(&wallet_address)
        .expect("Challenge creation should succeed");

    assert!(!challenge.nonce.is_empty());
    assert!(challenge.message.contains(&wallet_address));
    assert!(challenge.message.contains(&challenge.nonce));

    // 2. Client signs challenge message
    let signature = signer
        .sign_message(&challenge.message)
        .expect("Signing should succeed");

    // 3. Verify signature
    let (user, token, exp_ms) = service
        .verify(&wallet_address, &challenge.nonce, &signature)
        .expect("Signature verification should succeed");

    assert_eq!(user.providers, vec!["solana".to_string()]);
    assert!(!token.is_empty());
    assert!(exp_ms > 0);

    // 4. Replay attack: verifying again must fail
    let replay_err = service.verify(&wallet_address, &challenge.nonce, &signature);
    assert!(matches!(
        replay_err,
        Err(SolanaAuthError::InvalidOrExpiredChallenge)
    ));
}

#[test]
fn test_solana_tampered_signature_rejection() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");
    let service = SolanaAuthService::new(db, TokenManager::default());

    let signer = SolanaSigner::new_ephemeral();
    let wallet_address = signer.public_key_b58();

    let challenge = service
        .create_challenge(&wallet_address)
        .expect("Challenge creation should succeed");

    // Sign a DIFFERENT message (e.g. attacker attempting to replay another signature)
    let bad_signature = signer
        .sign_message("Forged message text")
        .expect("Signing should succeed");

    let verify_err = service.verify(&wallet_address, &challenge.nonce, &bad_signature);
    assert!(matches!(verify_err, Err(SolanaAuthError::InvalidSignature)));
}

#[test]
fn test_solana_invalid_wallet_rejection() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");
    let service = SolanaAuthService::new(db, TokenManager::default());

    let bad_wallet = "not_a_valid_solana_wallet";
    let err = service.create_challenge(bad_wallet);
    assert!(matches!(err, Err(SolanaAuthError::InvalidWalletAddress)));
}
