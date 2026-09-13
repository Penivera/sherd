import hashlib
import secrets
import uuid
from datetime import datetime, timedelta, timezone
from urllib.parse import urlparse

from fastapi import HTTPException, status
from jose import JWTError, jwt
from passlib.context import CryptContext

from app.config import settings

pwd_context = CryptContext(schemes=["argon2"], deprecated="auto")


def hash_password(password: str) -> str:
    return pwd_context.hash(password)


def verify_password(plain_password: str, password_hash: str) -> bool:
    return pwd_context.verify(plain_password, password_hash)


def create_access_token(user_id: uuid.UUID) -> str:
    expire = datetime.now(timezone.utc) + timedelta(minutes=settings.jwt_expire_minutes)
    payload = {"sub": str(user_id), "exp": expire}
    return jwt.encode(payload, settings.jwt_secret_key, algorithm=settings.jwt_algorithm)


def decode_access_token(token: str) -> uuid.UUID | None:
    try:
        payload = jwt.decode(
            token, settings.jwt_secret_key, algorithms=[settings.jwt_algorithm]
        )
    except JWTError:
        return None
    sub = payload.get("sub")
    if sub is None:
        return None
    try:
        return uuid.UUID(sub)
    except ValueError:
        return None


def generate_secure_token(num_bytes: int = 32) -> str:
    """URL-safe, unpredictable random token for nonces / one-time codes."""
    return secrets.token_urlsafe(num_bytes)


def hash_token(token: str) -> str:
    """One-way hash for storing single-use tokens/codes at rest."""
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def validate_desktop_redirect_uri(redirect_uri: str) -> str:
    """
    Restrict the desktop app's loopback callback to actual loopback
    addresses. Without this, the OAuth login/callback endpoints could be
    tricked into redirecting a one-time exchange code to an
    attacker-controlled host.
    """
    parsed = urlparse(redirect_uri)
    if parsed.scheme not in ("http", "https"):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="desktop_redirect_uri must be an http(s) loopback URL",
        )
    if parsed.hostname not in settings.allowed_desktop_redirect_hosts:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="desktop_redirect_uri must point at a loopback address",
        )
    return redirect_uri
