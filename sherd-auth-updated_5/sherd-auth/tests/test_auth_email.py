def test_register_success(client):
    resp = client.post(
        "/auth/register", json={"email": "a@example.com", "password": "supersecret"}
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["user"]["email"] == "a@example.com"
    assert body["user"]["providers"] == ["email"]
    assert body["access_token"]


def test_register_duplicate_email(client):
    client.post("/auth/register", json={"email": "dupe@example.com", "password": "supersecret"})
    resp = client.post(
        "/auth/register", json={"email": "dupe@example.com", "password": "anotherpass"}
    )
    assert resp.status_code == 409


def test_login_success(client):
    client.post("/auth/register", json={"email": "b@example.com", "password": "supersecret"})
    resp = client.post("/auth/login", json={"email": "b@example.com", "password": "supersecret"})
    assert resp.status_code == 200
    assert resp.json()["access_token"]


def test_login_incorrect_password(client):
    client.post("/auth/register", json={"email": "c@example.com", "password": "supersecret"})
    resp = client.post("/auth/login", json={"email": "c@example.com", "password": "wrongpass"})
    assert resp.status_code == 401


def test_me_without_auth(client):
    resp = client.get("/auth/me")
    assert resp.status_code == 401


def test_me_with_valid_auth(client):
    register_resp = client.post(
        "/auth/register", json={"email": "d@example.com", "password": "supersecret"}
    )
    token = register_resp.json()["access_token"]
    resp = client.get("/auth/me", headers={"Authorization": f"Bearer {token}"})
    assert resp.status_code == 200
    assert resp.json()["email"] == "d@example.com"
