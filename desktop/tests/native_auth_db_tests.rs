use desktop::auth::db::{AuthDb, DbError};

#[test]
fn test_db_user_creation_and_duplicate_prevention() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");

    let user = db
        .create_user_with_email("alice@example.com", "hash_alice_123")
        .expect("User creation should succeed");

    assert_eq!(user.email.as_deref(), Some("alice@example.com"));
    assert_eq!(user.providers, vec!["email".to_string()]);

    // Query back
    let fetched = db
        .get_user_by_email("alice@example.com")
        .expect("Fetch should succeed")
        .expect("User must exist");
    assert_eq!(fetched.id, user.id);
    assert_eq!(fetched.password_hash.as_deref(), Some("hash_alice_123"));

    // Duplicate email must fail
    let duplicate_result = db.create_user_with_email("alice@example.com", "another_hash");
    assert!(matches!(duplicate_result, Err(DbError::EmailAlreadyExists(_))));
}

#[test]
fn test_db_provider_identity_linking() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");

    // 1. Create a user via Google OAuth
    let user_google = db
        .find_or_create_user_from_provider(
            "google",
            "google_sub_101",
            Some("bob@example.com"),
            Some("Bob Builder"),
        )
        .expect("OAuth user creation should succeed");
    assert_eq!(user_google.providers, vec!["google".to_string()]);

    // 2. Link a GitHub identity to the same user (same email)
    let user_github = db
        .find_or_create_user_from_provider(
            "github",
            "github_id_202",
            Some("bob@example.com"),
            Some("bob_dev"),
        )
        .expect("Account linking should succeed");

    assert_eq!(user_github.id, user_google.id, "Linked identities must resolve to the same user");
    assert_eq!(user_github.providers, vec!["github".to_string(), "google".to_string()]);

    // 3. Existing identity lookup
    let user_existing = db
        .find_or_create_user_from_provider(
            "google",
            "google_sub_101",
            None,
            None,
        )
        .expect("Existing lookup should succeed");
    assert_eq!(user_existing.id, user_google.id);
}

#[test]
fn test_db_solana_challenge_lifecycle_and_replay_protection() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");
    let wallet = "4Nd1mBQtrMJVYVfKf2PJy9NZNLd3FFANGibZF93e1Dja";
    let nonce = "test_nonce_777";
    let message = "Sign in to Sherd message";
    let expires_at = "2099-01-01T00:00:00Z";
    let now = "2026-09-20T12:00:00Z";

    db.create_solana_challenge(wallet, nonce, message, expires_at)
        .expect("Challenge creation should succeed");

    // Consume challenge
    let consumed_msg = db
        .consume_solana_challenge(nonce, wallet, now)
        .expect("Consume call should succeed");
    assert_eq!(consumed_msg.as_deref(), Some(message));

    // Replay attack: consuming a second time must return None
    let replay_result = db
        .consume_solana_challenge(nonce, wallet, now)
        .expect("Second consume call should succeed");
    assert!(replay_result.is_none(), "Replayed challenge must be rejected");

    // Consume with wrong wallet must return None
    let wrong_wallet = "5Nd1mBQtrMJVYVfKf2PJy9NZNLd3FFANGibZF93e1Djb";
    let wrong_result = db
        .consume_solana_challenge(nonce, wrong_wallet, now)
        .expect("Wrong wallet consume call should succeed");
    assert!(wrong_result.is_none(), "Wrong wallet must be rejected");
}

#[test]
fn test_db_oauth_exchange_code_atomic_consumption() {
    let db = AuthDb::open_in_memory().expect("Failed to open in-memory db");
    let user = db
        .create_user_with_email("user@example.com", "hash")
        .expect("User creation should succeed");

    let code_hash = "sha256_hash_of_one_time_code";
    let expires_at = "2099-01-01T00:00:00Z";
    let now = "2026-09-20T12:00:00Z";

    db.create_oauth_exchange_code(&user.id, code_hash, expires_at)
        .expect("Code creation should succeed");

    // First consumption succeeds
    let redeemed = db
        .consume_oauth_exchange_code(code_hash, now)
        .expect("Consume should succeed");
    assert!(redeemed.is_some());
    assert_eq!(redeemed.unwrap().id, user.id);

    // Second consumption fails (atomic single-use)
    let replayed = db
        .consume_oauth_exchange_code(code_hash, now)
        .expect("Replay check should succeed");
    assert!(replayed.is_none(), "Single-use code cannot be reused");
}
