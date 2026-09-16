from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy.orm import Session

from app.database import get_db
from app.oauth_common import consume_exchange_code
from app.schemas import OAuthExchangeRequest, TokenResponse
from app.security import create_access_token
from app.serializers import to_user_out

router = APIRouter(tags=["auth-token"])


@router.post("/auth/token/exchange", response_model=TokenResponse)
def exchange_code(payload: OAuthExchangeRequest, db: Session = Depends(get_db)) -> TokenResponse:
    """
    Redeems the one-time code the desktop app received on its loopback
    callback after a Google/GitHub login, in exchange for the real
    session JWT. The code is single-use and short-lived (see
    Settings.oauth_exchange_code_ttl_seconds).
    """
    user = consume_exchange_code(db, payload.code)
    if user is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid, expired, or already-used exchange code",
        )
    token = create_access_token(user.id)
    return TokenResponse(access_token=token, user=to_user_out(user))
