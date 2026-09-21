use desktop::auth::db::AuthDb;
use desktop::auth::email::{EmailAuthError, EmailAuthService};
use desktop::auth::token::TokenManager;

#[tokio::test]
async fn test_argon2_password_hashing_and_verification() {
    let password = "SuperSecretPassword123!";
    let hash = EmailAuthService::hash_password(password).expect("Hashing should succeed");

    assert!(hash.starts_with("$argon2"), "Hash must use Argon2 format");
    assert!(EmailAuthService::verify_password(password, &hash));
    assert!(!EmailAuthService::verify_password("WrongPassword!", &hash));
}

#[tokio::test]
async fn test_email_registration_and_validation() {
    let db = AuthDb::open_in_memory().await.expect("Failed to open in-memory db");
    let service = EmailAuthService::new(db, TokenManager::default());

    // Password too short
    let short_pw_err = service.register("valid@example.com", "short").await;
    assert!(matches!(short_pw_err, Err(EmailAuthError::PasswordTooShort)));

    // Invalid email
    let invalid_email_err = service.register("not-an-email", "validpassword123").await;
    assert!(matches!(invalid_email_err, Err(EmailAuthError::InvalidEmail)));

    // Successful registration
    let (user, token, exp_ms) = service
        .register("ValidUser@Example.COM", "validpassword123")
        .await
        .expect("Registration should succeed");

    assert_eq!(user.email.as_deref(), Some("validuser@example.com"));
    assert!(!token.is_empty(), "Token must be non-empty JWT");
    assert!(exp_ms > 0);

    // Duplicate registration must fail
    let duplicate_err = service.register("validuser@example.com", "validpassword123").await;
    assert!(matches!(duplicate_err, Err(EmailAuthError::EmailAlreadyExists)));
}

#[tokio::test]
async fn test_email_login_flow() {
    let db = AuthDb::open_in_memory().await.expect("Failed to open in-memory db");
    let service = EmailAuthService::new(db, TokenManager::default());

    let email = "login_test@example.com";
    let password = "Password456!";

    service.register(email, password).await.expect("Registration should succeed");

    // Successful login
    let (user, token, _) = service
        .login(email, password).await
        .expect("Login should succeed");
    assert_eq!(user.email.as_deref(), Some(email));
    assert!(!token.is_empty());

    // Wrong password
    let wrong_pw = service.login(email, "WrongPassword!").await;
    assert!(matches!(wrong_pw, Err(EmailAuthError::InvalidCredentials)));

    // Non-existent user
    let no_user = service.login("unknown@example.com", password).await;
    assert!(matches!(no_user, Err(EmailAuthError::InvalidCredentials)));
}
