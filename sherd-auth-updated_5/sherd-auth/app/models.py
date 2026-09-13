import enum
import uuid
from datetime import datetime, timezone

from sqlalchemy import DateTime, Enum, ForeignKey, String, UniqueConstraint
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base
from app.db_types import GUID


def _now() -> datetime:
    return datetime.now(timezone.utc)


class AuthProvider(str, enum.Enum):
    email = "email"
    google = "google"
    github = "github"
    solana = "solana"


class User(Base):
    __tablename__ = "users"

    id: Mapped[uuid.UUID] = mapped_column(
        GUID(), primary_key=True, default=uuid.uuid4
    )
    email: Mapped[str | None] = mapped_column(String(320), unique=True, nullable=True)
    password_hash: Mapped[str | None] = mapped_column(String(255), nullable=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=_now)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_now, onupdate=_now
    )

    identities: Mapped[list["AuthIdentity"]] = relationship(
        back_populates="user", cascade="all, delete-orphan"
    )


class AuthIdentity(Base):
    """
    Links one external (or internal) auth method to a single Sherd User.
    A user can have multiple identities (email, google, github, solana),
    but each (provider, provider_account_id) pair must be globally unique.
    """

    __tablename__ = "auth_identities"
    __table_args__ = (
        UniqueConstraint("provider", "provider_account_id", name="uq_provider_account"),
    )

    id: Mapped[uuid.UUID] = mapped_column(
        GUID(), primary_key=True, default=uuid.uuid4
    )
    user_id: Mapped[uuid.UUID] = mapped_column(
        GUID(), ForeignKey("users.id", ondelete="CASCADE"), nullable=False
    )
    provider: Mapped[AuthProvider] = mapped_column(Enum(AuthProvider), nullable=False)

    # For email: user's email. For google/github: their subject/user id.
    # For solana: the base58 wallet public key.
    provider_account_id: Mapped[str] = mapped_column(String(255), nullable=False)

    # Optional extra info (e.g. github username, google display name). Kept
    # generic and non-sensitive; never store tokens/secrets here.
    provider_metadata: Mapped[str | None] = mapped_column(String(500), nullable=True)

    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=_now)

    user: Mapped["User"] = relationship(back_populates="identities")


class SolanaChallenge(Base):
    """
    A server-issued, single-use, expiring challenge that a wallet must sign
    to prove ownership before a Solana identity is linked/logged in.

    `message` is the exact text the wallet must sign, generated and stored
    server-side at challenge-creation time. Verification never trusts a
    client-supplied message string; it always re-checks the signature
    against this stored value, so a client cannot get an arbitrary message
    signed and passed off as a valid login challenge.
    """

    __tablename__ = "solana_challenges"

    id: Mapped[uuid.UUID] = mapped_column(GUID(), primary_key=True, default=uuid.uuid4)
    wallet_address: Mapped[str] = mapped_column(String(64), nullable=False, index=True)
    nonce: Mapped[str] = mapped_column(String(64), nullable=False, unique=True, index=True)
    message: Mapped[str] = mapped_column(String(512), nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=_now)
    expires_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)
    consumed_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True), nullable=True
    )


class OAuthExchangeCode(Base):
    """
    A short-lived, single-use code minted right after a Google/GitHub OAuth
    callback finishes, so the browser redirect back to the desktop app's
    loopback listener carries a throwaway code instead of the actual JWT.
    The desktop app immediately exchanges this code for the real session
    token via POST /auth/token/exchange.

    Only a hash of the code is stored, the same way a password would be, so
    a leaked database dump (logs, backups) can't be used to mint sessions.
    """

    __tablename__ = "oauth_exchange_codes"

    id: Mapped[uuid.UUID] = mapped_column(GUID(), primary_key=True, default=uuid.uuid4)
    code_hash: Mapped[str] = mapped_column(String(64), nullable=False, unique=True, index=True)
    user_id: Mapped[uuid.UUID] = mapped_column(
        GUID(), ForeignKey("users.id", ondelete="CASCADE"), nullable=False
    )
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=_now)
    expires_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)
    consumed_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True), nullable=True
    )
