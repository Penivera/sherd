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

router = APIRouter(tags=["auth-github"])


@router.get("/auth/github/login")
async def github_login(
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
    request.session["desktop_redirect_uri"] = desktop_redirect_uri
    return await oauth.github.authorize_redirect(request, settings.github_redirect_uri)


@router.get("/auth/github/callback")
async def github_callback(request: Request, db: Session = Depends(get_db)):
    try:
        token = await oauth.github.authorize_access_token(request)
    except Exception as exc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST, detail=f"GitHub OAuth failed: {exc}"
        )

    profile_resp = await oauth.github.get("user", token=token)
    profile_resp.raise_for_status()
    profile = profile_resp.json()

    github_id = str(profile["id"])
    email = profile.get("email")

    # GitHub often omits email from /user if it's not public; fall back to
    # the dedicated emails endpoint and use the primary verified address.
    if not email:
        emails_resp = await oauth.github.get("user/emails", token=token)
        if emails_resp.status_code == 200:
            for entry in emails_resp.json():
                if entry.get("primary") and entry.get("verified"):
                    email = entry.get("email")
                    break

    user = find_or_create_user_from_oauth(
        db=db,
        provider=AuthProvider.github,
        provider_account_id=github_id,
        email=email,
        metadata=profile.get("login"),
    )

    desktop_redirect_uri = request.session.pop("desktop_redirect_uri", None)
    if desktop_redirect_uri:
        code = create_exchange_code(db, user.id)
        return RedirectResponse(f"{desktop_redirect_uri}?code={code}")

    app_token = create_access_token(user.id)
    return TokenResponse(access_token=app_token, user=to_user_out(user))
