import time

from nacl.signing import SigningKey

from tests.conftest import sign_message


def get_challenge(client, wallet_address):
    resp = client.post("/auth/solana/challenge", json={"wallet_address": wallet_address})
    assert resp.status_code == 200
    return resp.json()


def test_valid_solana_signature_logs_in(client, solana_wallet):
    public_key_b58, signing_key = solana_wallet
    challenge = get_challenge(client, public_key_b58)
    signature = sign_message(signing_key, challenge["message"])

    resp = client.post(
        "/auth/solana/verify",
        json={
            "wallet_address": public_key_b58,
            "nonce": challenge["nonce"],
            "signature": signature,
        },
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["user"]["providers"] == ["solana"]


def test_invalid_solana_signature_rejected(client, solana_wallet):
    public_key_b58, signing_key = solana_wallet
    other_key = SigningKey.generate()
    challenge = get_challenge(client, public_key_b58)
    # Sign with the WRONG key; should fail verification against public_key_b58.
    bad_signature = sign_message(other_key, challenge["message"])

    resp = client.post(
        "/auth/solana/verify",
        json={
            "wallet_address": public_key_b58,
            "nonce": challenge["nonce"],
            "signature": bad_signature,
        },
    )
    assert resp.status_code == 401


def test_same_wallet_reuses_same_user(client, solana_wallet):
    public_key_b58, signing_key = solana_wallet

    challenge1 = get_challenge(client, public_key_b58)
    sig1 = sign_message(signing_key, challenge1["message"])
    first = client.post(
        "/auth/solana/verify",
        json={"wallet_address": public_key_b58, "nonce": challenge1["nonce"], "signature": sig1},
    )

    challenge2 = get_challenge(client, public_key_b58)
    sig2 = sign_message(signing_key, challenge2["message"])
    second = client.post(
        "/auth/solana/verify",
        json={"wallet_address": public_key_b58, "nonce": challenge2["nonce"], "signature": sig2},
    )

    assert first.json()["user"]["id"] == second.json()["user"]["id"]


def test_malformed_wallet_address_rejected(client):
    resp = client.post(
        "/auth/solana/challenge", json={"wallet_address": "not-base58!!!"}
    )
    assert resp.status_code == 400


def test_reused_challenge_rejected(client, solana_wallet):
    public_key_b58, signing_key = solana_wallet
    challenge = get_challenge(client, public_key_b58)
    signature = sign_message(signing_key, challenge["message"])
    payload = {
        "wallet_address": public_key_b58,
        "nonce": challenge["nonce"],
        "signature": signature,
    }

    first = client.post("/auth/solana/verify", json=payload)
    assert first.status_code == 200

    second = client.post("/auth/solana/verify", json=payload)
    assert second.status_code == 401


def test_expired_challenge_rejected(client, solana_wallet, monkeypatch):
    from app import config as config_module

    monkeypatch.setattr(config_module.settings, "solana_challenge_ttl_seconds", 0)

    public_key_b58, signing_key = solana_wallet
    challenge = get_challenge(client, public_key_b58)
    time.sleep(0.01)  # ensure we're past the (zero-second) expiry
    signature = sign_message(signing_key, challenge["message"])

    resp = client.post(
        "/auth/solana/verify",
        json={
            "wallet_address": public_key_b58,
            "nonce": challenge["nonce"],
            "signature": signature,
        },
    )
    assert resp.status_code == 401


def test_wrong_wallet_for_challenge_rejected(client, solana_wallet):
    """A challenge issued for one wallet can't be redeemed under a different wallet_address."""
    public_key_b58, signing_key = solana_wallet
    other_key = SigningKey.generate()
    import base58

    other_wallet = base58.b58encode(bytes(other_key.verify_key)).decode()

    challenge = get_challenge(client, public_key_b58)
    # Even a technically-valid signature from the OTHER wallet over this
    # challenge's message must be rejected, since the challenge is bound
    # to public_key_b58, not other_wallet.
    signature = sign_message(other_key, challenge["message"])

    resp = client.post(
        "/auth/solana/verify",
        json={
            "wallet_address": other_wallet,
            "nonce": challenge["nonce"],
            "signature": signature,
        },
    )
    assert resp.status_code == 401


def test_unknown_nonce_rejected(client, solana_wallet):
    public_key_b58, signing_key = solana_wallet
    signature = sign_message(signing_key, "arbitrary message")

    resp = client.post(
        "/auth/solana/verify",
        json={
            "wallet_address": public_key_b58,
            "nonce": "this-nonce-was-never-issued",
            "signature": signature,
        },
    )
    assert resp.status_code == 401
