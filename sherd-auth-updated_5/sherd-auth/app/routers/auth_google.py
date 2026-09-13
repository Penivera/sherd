from fastapi import APIRouter, Depends, HTTPException, Query, Request, status
from fastapi.responses import RedirectResponse
from sqlalchemy.orm import Session

from app.config import settings
from app.database import get_db
from app.models import AuthProvider
from app.oauth_clients import oauth
from app.oauth_common import create_exchange_code, find_or_create_user_from_oauth
from app.schemas import TokenResponse
from app.security import create_access_token, validate_desktop_redirect_uri
from app.serializers import to_user_out

router = APIRouter(tags=["auth-google"])


@router.get("/auth/google/login")
async def google_login(
    request: Request,
    desktop_redirect_uri: str | None = Query(
        default=None,
        description=(
            "Loopback URL the desktop app is listening on "
            "(e.g. http://127.0.0.1:53214/callback). When provided, "
            "the callback hands back a one-time code here instead of "
            "returning the session token directly."
        ),
    ),
):
    if desktop_redirect_uri is not None:
        validate_desktop_redirect_uri(desktop_redirect_uri)
    # Stashed in the signed session cookie set by SessionMiddleware; carried
    # through the round trip to Google and back by the same browser.
    request.session["desktop_redirect_uri"] = desktop_redirect_uri
    return await oauth.google.authorize_redirect(request, settings.google_redirect_uri)


@router.get("/auth/google/callback")
async def google_callback(request: Request, db: Session = Depends(get_db)):
    try:
        token = await oauth.google.authorize_access_token(request)
    except Exception as exc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST, detail=f"Google OAuth failed: {exc}"
        )

    userinfo = token.get("userinfo")
    if userinfo is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Google did not return user info",
        )

    google_sub = userinfo["sub"]
    email = userinfo.get("email")

    user = find_or_create_user_from_oauth(
        db=db,
        provider=AuthProvider.google,
        provider_account_id=google_sub,
        email=email,
        metadata=userinfo.get("name"),
    )

    desktop_redirect_uri = request.session.pop("desktop_redirect_uri", None)
    if desktop_redirect_uri:
        code = create_exchange_code(db, user.id)
        return RedirectResponse(f"{desktop_redirect_uri}?code={code}")

    # No loopback registered (e.g. hitting this route directly in a
    # browser for manual testing) -> return the token directly.
    app_token = create_access_token(user.id)
    return TokenResponse(access_token=app_token, user=to_user_out(user))
