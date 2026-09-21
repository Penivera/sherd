use desktop::auth::manager::AuthManager;
use desktop::auth::wallet::SolanaSigner;

#[tokio::test]
async fn test_auth_manager_email_flow_and_session_persistence() {
    let manager = AuthManager::in_memory_with_account("test_email_flow_account")
        .expect("Failed to initialize in-memory AuthManager");
    let _ = manager.logout().await;

    // 1. Initially no session
    assert!(manager.load_cached_session().is_none());

    // 2. Register
    let email = "manager_test@example.com";
    let password = "SecretPassword789!";
    let session = manager
        .register_email(email, password)
        .await
        .expect("Registration should succeed");

    assert_eq!(session.user.email.as_deref(), Some(email));
    assert!(!session.access_token.is_empty());

    // 3. Session was automatically persisted to storage
    let cached = manager
        .load_cached_session()
        .expect("Session must be cached");
    assert_eq!(cached.access_token, session.access_token);
    assert_eq!(cached.user.id, session.user.id);
    assert!(!cached.is_expired());

    // 4. Logout clears storage
    manager.logout().await.expect("Logout should succeed");
    assert!(manager.load_cached_session().is_none());

    // 5. Login restores session
    let login_session = manager
        .login_email(email, password)
        .await
        .expect("Login should succeed");
    assert_eq!(login_session.user.id, session.user.id);

    let cached_after_login = manager
        .load_cached_session()
        .expect("Session must be cached after login");
    assert_eq!(cached_after_login.access_token, login_session.access_token);

    // Clean up
    let _ = manager.logout().await;
}

#[tokio::test]
async fn test_auth_manager_solana_flow_and_session_persistence() {
    let manager = AuthManager::in_memory_with_account("test_solana_flow_account")
        .expect("Failed to initialize in-memory AuthManager");
    let _ = manager.logout().await;

    let signer = SolanaSigner::new_ephemeral();
    let wallet_address = signer.public_key_b58();

    let session = manager
        .login_solana_with_signer(&signer)
        .await
        .expect("Solana authentication should succeed");

    assert_eq!(session.user.providers, vec!["solana".to_string()]);
    assert!(!session.access_token.is_empty());

    // Verified cached session
    let cached = manager
        .load_cached_session()
        .expect("Session must be cached");
    assert_eq!(cached.access_token, session.access_token);
    assert!(!cached.is_expired());
    assert_eq!(
        signer.public_key_b58(),
        wallet_address,
        "Signer public key must match"
    );

    // Clean up
    let _ = manager.logout().await;
}
