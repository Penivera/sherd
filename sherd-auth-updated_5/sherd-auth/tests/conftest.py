import os

os.environ.setdefault("DATABASE_URL", "sqlite:///:memory:")
os.environ.setdefault("JWT_SECRET_KEY", "test-secret")

import base58
import pytest
from fastapi.testclient import TestClient
from nacl.signing import SigningKey
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker
from sqlalchemy.pool import StaticPool

from app.database import Base, get_db
from app.main import app


@pytest.fixture()
def db_session():
    engine = create_engine(
        "sqlite:///:memory:",
        connect_args={"check_same_thread": False},
        poolclass=StaticPool,
    )
    Base.metadata.create_all(engine)
    TestingSessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)
    session = TestingSessionLocal()
    try:
        yield session
    finally:
        session.close()
        Base.metadata.drop_all(engine)


@pytest.fixture()
def client(db_session):
    def override_get_db():
        try:
            yield db_session
        finally:
            pass

    app.dependency_overrides[get_db] = override_get_db
    with TestClient(app) as test_client:
        yield test_client
    app.dependency_overrides.clear()


@pytest.fixture()
def solana_wallet():
    """Returns (base58_public_key, signing_key) for a throwaway keypair."""
    signing_key = SigningKey.generate()
    public_key_b58 = base58.b58encode(bytes(signing_key.verify_key)).decode()
    return public_key_b58, signing_key


def sign_message(signing_key, message: str) -> str:
    signed = signing_key.sign(message.encode("utf-8"))
    return base58.b58encode(signed.signature).decode()
