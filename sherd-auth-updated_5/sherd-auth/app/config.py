from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    database_url: str

    jwt_secret_key: str
    jwt_algorithm: str = "HS256"
    jwt_expire_minutes: int = 60

    google_client_id: str = ""
    google_client_secret: str = ""
    google_redirect_uri: str = "http://localhost:8000/auth/google/callback"

    github_client_id: str = ""
    github_client_secret: str = ""
    github_redirect_uri: str = "http://localhost:8000/auth/github/callback"

    frontend_url: str = "http://localhost:3000"

    # --- Solana challenge/nonce auth ---
    # How long a generated challenge remains valid before it must be rejected.
    solana_challenge_ttl_seconds: int = 300

    # --- Desktop OAuth loopback handoff ---
    # After Google/GitHub OAuth completes server-side, we redirect the
    # system browser back to a loopback URL the desktop app is listening
    # on, with a short-lived one-time code (never the JWT itself, since a
    # code fits in a URL/browser-history-visible redirect more safely than
    # a long-lived credential would).
    oauth_exchange_code_ttl_seconds: int = 60
    # Hosts the desktop app is allowed to register as its loopback callback.
    # Anything else is rejected to prevent the login flow being hijacked
    # into redirecting a real token/code to an attacker-controlled host.
    allowed_desktop_redirect_hosts: tuple[str, ...] = ("127.0.0.1", "localhost", "::1")


settings = Settings()
