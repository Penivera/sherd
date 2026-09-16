from datetime import datetime, timedelta, timezone

import base58
from fastapi import APIRouter, Depends, HTTPException, status
from nacl.exceptions import BadSignatureError
from nacl.signing import VerifyKey
from sqlalchemy import update
from sqlalchemy.orm import Session

from app.config import settings
from app.database import get_db
from app.models import AuthProvider, SolanaChallenge
from app.oauth_common import find_or_create_user_from_oauth
from app.schemas import SolanaChallengeRequest, SolanaChallengeResponse, SolanaVerifyRequest, TokenResponse
from app.security import create_access_token, generate_secure_token
from app.serializers import to_user_out

router = APIRouter(tags=["auth-solana"])


def _as_aware_utc(value: datetime) -> datetime:
    # SQLite (used in tests) doesn't round-trip tzinfo on DateTime(timezone=True)
    # columns, so a value read back from the DB can come back naive even
    # though it was stored as UTC. Postgres (production) preserves it, so
    # this is a no-op there; here it just avoids naive/aware comparison
    # errors in tests.
    if value.tzinfo is None:
        return value.replace(tzinfo=timezone.utc)
    return value


def _is_valid_solana_address(wallet_address: str) -> bool:
    try:
        return len(base58.b58decode(wallet_address)) == 32
    except ValueError:
        return False


def _build_challenge_message(wallet_address: str, nonce: str, issued_at: datetime, expires_at: datetime) -> str:
    # Everything the wallet is asked to sign is generated and stored
    # server-side. The client never gets to choose or influence this text,
    # which is what stops a captured signature over some other message
    # being replayed here as if it were a login.
    return (
        "Sign in to Sherd\n\n"
        f"Wallet: {wallet_address}\n"
        f"Nonce: {nonce}\n"
        f"Issued At: {issued_at.isoformat()}\n"
        f"Expires At: {expires_at.isoformat()}"
    )


def _verify_solana_signature(wallet_address: str, message: str, signature: str) -> bool:
    try:
        public_key_bytes = base58.b58decode(wallet_address)
        signature_bytes = base58.b58decode(signature)
    except ValueError:
        return False

    if len(public_key_bytes) != 32:
        return False

    try:
        verify_key = VerifyKey(public_key_bytes)
        verify_key.verify(message.encode("utf-8"), signature_bytes)
        return True
    except BadSignatureError:
        return False
    except Exception:
        return False


@router.post("/auth/solana/challenge", response_model=SolanaChallengeResponse)
def solana_challenge(payload: SolanaChallengeRequest, db: Session = Depends(get_db)) -> SolanaChallengeResponse:
    if not _is_valid_solana_address(payload.wallet_address):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST, detail="Invalid Solana wallet address"
        )

    nonce = generate_secure_token(16)
    issued_at = datetime.now(timezone.utc)
    expires_at = issued_at + timedelta(seconds=settings.solana_challenge_ttl_seconds)
    message = _build_challenge_message(payload.wallet_address, nonce, issued_at, expires_at)

    db.add(
        SolanaChallenge(
            wallet_address=payload.wallet_address,
            nonce=nonce,
            message=message,
            expires_at=expires_at,
        )
    )
    db.commit()

    return SolanaChallengeResponse(nonce=nonce, message=message, expires_at=expires_at)


@router.post("/auth/solana/verify", response_model=TokenResponse)
def solana_verify(payload: SolanaVerifyRequest, db: Session = Depends(get_db)) -> TokenResponse:
    challenge = db.query(SolanaChallenge).filter(SolanaChallenge.nonce == payload.nonce).first()

    invalid_challenge = HTTPException(
        status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid, expired, or already-used challenge"
    )

    if challenge is None:
        raise invalid_challenge

    now = datetime.now(timezone.utc)
    if challenge.consumed_at is not None or _as_aware_utc(challenge.expires_at) <= now:
        raise invalid_challenge

    # The challenge is bound to the wallet address it was issued for; a
    # signature that's otherwise valid but for a different wallet than the
    # one that requested this nonce is rejected.
    if challenge.wallet_address != payload.wallet_address:
        raise invalid_challenge

    if not _verify_solana_signature(challenge.wallet_address, challenge.message, payload.signature):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid wallet signature"
        )

    # Atomically consume: the UPDATE only affects a row that is still
    # unconsumed and unexpired, so two concurrent verify attempts against
    # the same nonce can't both succeed (single-use is enforced at the DB
    # level, not just by this request's earlier read).
    result = db.execute(
        update(SolanaChallenge)
        .where(
            SolanaChallenge.id == challenge.id,
            SolanaChallenge.consumed_at.is_(None),
            SolanaChallenge.expires_at > now,
        )
        .values(consumed_at=now)
        .execution_options(synchronize_session=False)
    )
    db.commit()
    if result.rowcount != 1:
        raise invalid_challenge

    user = find_or_create_user_from_oauth(
        db=db,
        provider=AuthProvider.solana,
        provider_account_id=payload.wallet_address,
        email=None,
    )

    token = create_access_token(user.id)
    return TokenResponse(access_token=token, user=to_user_out(user))
