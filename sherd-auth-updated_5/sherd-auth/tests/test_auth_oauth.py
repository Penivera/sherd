from unittest.mock import AsyncMock, MagicMock

import pytest

from app.oauth_clients import oauth


def test_google_callback_creates_user(client, monkeypatch):
    fake_token = {
        "userinfo": {
            "sub": "google-user-123",
            "email": "googleuser@example.com",
            "name": "Google User",
        }
    }
    monkeypatch.setattr(
        oauth.google, "authorize_access_token", AsyncMock(return_value=fake_token)
    )

    resp = client.get("/auth/google/callback")
    assert resp.status_code == 200
    body = resp.json()
    assert body["user"]["email"] == "googleuser@example.com"
    assert body["user"]["providers"] == ["google"]


def test_google_callback_invalid_state_returns_400(client, monkeypatch):
    async def raise_error(request):
        raise ValueError("mismatching_state: CSRF Warning!")

    monkeypatch.setattr(oauth.google, "authorize_access_token", raise_error)

    resp = client.get("/auth/google/callback")
    assert resp.status_code == 400


def test_github_callback_creates_user(client, monkeypatch):
    monkeypatch.setattr(
        oauth.github, "authorize_access_token", AsyncMock(return_value={"access_token": "x"})
    )

    profile_response = MagicMock()
    profile_response.raise_for_status = MagicMock()
    profile_response.json = MagicMock(
        return_value={"id": 4242, "login": "octocat", "email": "octocat@example.com"}
    )
    monkeypatch.setattr(oauth.github, "get", AsyncMock(return_value=profile_response))

    resp = client.get("/auth/github/callback")
    assert resp.status_code == 200
    body = resp.json()
    assert body["user"]["email"] == "octocat@example.com"
    assert body["user"]["providers"] == ["github"]


def test_github_callback_expired_code_returns_400(client, monkeypatch):
    async def raise_error(request):
        raise ValueError("bad_verification_code")

    monkeypatch.setattr(oauth.github, "authorize_access_token", raise_error)

    resp = client.get("/auth/github/callback")
    assert resp.status_code == 400


def test_google_login_links_to_existing_email_user(client, monkeypatch):
    # A user already registered via email/password...
    register_resp = client.post(
        "/auth/register", json={"email": "shared@example.com", "password": "supersecret"}
    )
    existing_user_id = register_resp.json()["user"]["id"]

    # ...then logs in with Google using the SAME email. Should link to the
    # same user rather than creating a second one.
    fake_token = {
        "userinfo": {"sub": "google-shared-1", "email": "shared@example.com", "name": "Shared"}
    }
    monkeypatch.setattr(
        oauth.google, "authorize_access_token", AsyncMock(return_value=fake_token)
    )

    resp = client.get("/auth/google/callback")
    body = resp.json()
    assert body["user"]["id"] == existing_user_id
    assert set(body["user"]["providers"]) == {"email", "google"}


def test_repeated_google_login_does_not_duplicate_identity(client, monkeypatch):
    fake_token = {
        "userinfo": {"sub": "google-repeat-1", "email": "repeat@example.com", "name": "R"}
    }
    monkeypatch.setattr(
        oauth.google, "authorize_access_token", AsyncMock(return_value=fake_token)
    )

    first = client.get("/auth/google/callback")
    second = client.get("/auth/google/callback")

    assert first.json()["user"]["id"] == second.json()["user"]["id"]
    assert second.json()["user"]["providers"] == ["google"]


def test_google_login_rejects_non_loopback_redirect(client):
    resp = client.get(
        "/auth/google/login",
        params={"desktop_redirect_uri": "http://evil.example.com/callback"},
        follow_redirects=False,
    )
    assert resp.status_code == 400


def test_desktop_loopback_handoff_then_exchange(client, monkeypatch):
    """
    Full desktop flow: /login stashes the loopback redirect in the
    session, /callback hands back a one-time code instead of the token,
    and the code is redeemable exactly once via /auth/token/exchange.
    """
    from starlette.responses import RedirectResponse

    # Avoid a real network call to Google's OIDC discovery document; we're
    # testing our own session-stashing + redirect behavior here, not
    # authlib's OAuth mechanics (covered by the mocked-callback tests above).
    monkeypatch.setattr(
        oauth.google,
        "authorize_redirect",
        AsyncMock(return_value=RedirectResponse("https://accounts.google.com/fake-authorize")),
    )

    login_resp = client.get(
        "/auth/google/login",
        params={"desktop_redirect_uri": "http://127.0.0.1:53214/callback"},
        follow_redirects=False,
    )
    assert login_resp.status_code in (302, 307)

    fake_token = {
        "userinfo": {"sub": "google-desktop-1", "email": "desktop@example.com", "name": "D"}
    }
    monkeypatch.setattr(
        oauth.google, "authorize_access_token", AsyncMock(return_value=fake_token)
    )

    callback_resp = client.get("/auth/google/callback", follow_redirects=False)
    assert callback_resp.status_code in (302, 307)
    location = callback_resp.headers["location"]
    assert location.startswith("http://127.0.0.1:53214/callback?code=")
    code = location.split("code=", 1)[1]

    exchange_resp = client.post("/auth/token/exchange", json={"code": code})
    assert exchange_resp.status_code == 200
    body = exchange_resp.json()
    assert body["user"]["email"] == "desktop@example.com"
    assert body["access_token"]

    # Single-use: redeeming the same code again must fail.
    replay_resp = client.post("/auth/token/exchange", json={"code": code})
    assert replay_resp.status_code == 401


def test_exchange_unknown_code_rejected(client):
    resp = client.post("/auth/token/exchange", json={"code": "not-a-real-code"})
    assert resp.status_code == 401
