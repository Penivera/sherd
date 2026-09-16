from authlib.integrations.starlette_client import OAuth

from app.config import settings

oauth = OAuth()

# Google: uses OIDC discovery so authlib pulls the current authorization/
# token/userinfo endpoints and JWKS directly from Google rather than us
# hard-coding them.
oauth.register(
    name="google",
    client_id=settings.google_client_id,
    client_secret=settings.google_client_secret,
    server_metadata_url="https://accounts.google.com/.well-known/openid-configuration",
    client_kwargs={"scope": "openid email profile"},
)

# GitHub has no OIDC discovery document; endpoints are its stable OAuth2 URLs.
oauth.register(
    name="github",
    client_id=settings.github_client_id,
    client_secret=settings.github_client_secret,
    access_token_url="https://github.com/login/oauth/access_token",
    authorize_url="https://github.com/login/oauth/authorize",
    api_base_url="https://api.github.com/",
    client_kwargs={"scope": "read:user user:email"},
)
