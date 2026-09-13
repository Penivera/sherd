from fastapi import FastAPI
from starlette.middleware.sessions import SessionMiddleware

from app.config import settings
from app.routers import auth_email, auth_github, auth_google, auth_solana, auth_token
from fastapi.middleware.cors import CORSMiddleware

app = FastAPI(title="Sherd Auth Service")

# Required by authlib's Starlette OAuth client to store the OAuth `state`
# between the /login redirect and the /callback request, and by us to
# stash the desktop app's loopback redirect URI across that same round trip.
app.add_middleware(SessionMiddleware, secret_key=settings.jwt_secret_key)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:5173"],
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(auth_email.router)
app.include_router(auth_google.router)
app.include_router(auth_github.router)
app.include_router(auth_solana.router)
app.include_router(auth_token.router)


@app.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok"}
