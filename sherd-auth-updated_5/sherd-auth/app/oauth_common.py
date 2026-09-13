from datetime import datetime, timedelta, timezone

from sqlalchemy import update
from sqlalchemy.orm import Session

from app.config import settings
from app.models import AuthIdentity, AuthProvider, OAuthExchangeCode, User
from app.security import generate_secure_token, hash_token


def find_or_create_user_from_oauth(
    db: Session,
    provider: AuthProvider,
    provider_account_id: str,
    email: str | None,
    metadata: str | None = None,
) -> User:
    """
    Resolve an OAuth callback (Google/GitHub) to a single Sherd User.

    Resolution order:
    1. An identity already exists for (provider, provider_account_id) -> use its user.
    2. No identity, but a user already exists with this email -> link the new
       identity to that existing user (account linking).
    3. Neither exists -> create a new user + identity.
    """
    identity = (
        db.query(AuthIdentity)
        .filter(
            AuthIdentity.provider == provider,
            AuthIdentity.provider_account_id == provider_account_id,
        )
        .first()
    )
    if identity is not None:
        return identity.user

    user = None
    if email:
        user = db.query(User).filter(User.email == email).first()

    if user is None:
        user = User(email=email)
        db.add(user)
        db.flush()

    new_identity = AuthIdentity(
        user_id=user.id,
        provider=provider,
        provider_account_id=provider_account_id,
        provider_metadata=metadata,
    )
    db.add(new_identity)
    db.commit()
    db.refresh(user)
    return user


def create_exchange_code(db: Session, user_id) -> str:
    """
    Mint a short-lived one-time code for the desktop loopback handoff.
    Only the hash is persisted; the raw code is returned once and never
    stored, the same pattern used for password reset tokens.
    """
    raw_code = generate_secure_token()
    expires_at = datetime.now(timezone.utc) + timedelta(
        seconds=settings.oauth_exchange_code_ttl_seconds
    )
    db.add(
        OAuthExchangeCode(
            code_hash=hash_token(raw_code),
            user_id=user_id,
            expires_at=expires_at,
        )
    )
    db.commit()
    return raw_code


def consume_exchange_code(db: Session, raw_code: str) -> User | None:
    """
    Atomically redeem a one-time OAuth exchange code. The UPDATE only
    affects a row that is still unconsumed and unexpired, so two
    concurrent redemption attempts can't both succeed (mirrors the same
    atomic-consume requirement as the Solana challenge flow).
    """
    now = datetime.now(timezone.utc)
    code_hash = hash_token(raw_code)

    result = db.execute(
        update(OAuthExchangeCode)
        .where(
            OAuthExchangeCode.code_hash == code_hash,
            OAuthExchangeCode.consumed_at.is_(None),
            OAuthExchangeCode.expires_at > now,
        )
        .values(consumed_at=now)
        .execution_options(synchronize_session=False)
    )
    db.commit()

    if result.rowcount != 1:
        return None

    row = db.query(OAuthExchangeCode).filter(OAuthExchangeCode.code_hash == code_hash).first()
    return db.get(User, row.user_id) if row else None
