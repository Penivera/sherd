use sherd_desktop::auth::storage::SecureSessionStore;
use sherd_desktop::state::{PersistedSession, UserProfile};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_persisted_session_serialization_and_expiry() {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let future_session = PersistedSession {
        access_token: "test-token-future".to_string(),
        expires_at: Some(now_ms + 3_600_000), // 1 hour in the future
        user: UserProfile {
            id: "user-123".to_string(),
            email: Some("test@example.com".to_string()),
            providers: vec!["email".to_string()],
        },
    };

    assert!(!future_session.is_expired());

    // Serialize and deserialize
    let json = serde_json::to_string(&future_session).expect("Failed to serialize session");
    let deserialized: PersistedSession =
        serde_json::from_str(&json).expect("Failed to deserialize session");

    assert_eq!(deserialized.access_token, "test-token-future");
    assert_eq!(deserialized.user.id, "user-123");
    assert_eq!(deserialized.user.providers, vec!["email".to_string()]);
    assert!(!deserialized.is_expired());

    // Expired session
    let expired_session = PersistedSession {
        access_token: "test-token-past".to_string(),
        expires_at: Some(now_ms - 60_000), // 1 minute in the past
        user: UserProfile {
            id: "user-456".to_string(),
            email: None,
            providers: vec!["solana".to_string()],
        },
    };

    assert!(expired_session.is_expired());

    // Session without expiry never expires
    let non_expiring_session = PersistedSession {
        access_token: "test-token-forever".to_string(),
        expires_at: None,
        user: UserProfile {
            id: "user-789".to_string(),
            email: None,
            providers: vec![],
        },
    };
    assert!(!non_expiring_session.is_expired());
}

#[test]
fn test_corrupted_session_deserialization() {
    let invalid_json = "{ invalid: json }";
    let result = serde_json::from_str::<PersistedSession>(invalid_json);
    assert!(result.is_err());
}

#[test]
fn test_session_store_instantiation() {
    let store = SecureSessionStore::new();
    assert_eq!(store.service_name(), "com.sherd.desktop");
    assert_eq!(store.account_name(), "current_session");
}
