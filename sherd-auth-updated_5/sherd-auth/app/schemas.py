import uuid
from datetime import datetime

from pydantic import BaseModel, EmailStr, Field


class UserOut(BaseModel):
    id: uuid.UUID
    email: str | None
    providers: list[str]

    model_config = {"from_attributes": True}


class RegisterRequest(BaseModel):
    email: EmailStr
    password: str = Field(min_length=8, max_length=128)


class LoginRequest(BaseModel):
    email: EmailStr
    password: str


class TokenResponse(BaseModel):
    access_token: str
    token_type: str = "bearer"
    user: UserOut


class SolanaChallengeRequest(BaseModel):
    wallet_address: str = Field(description="Base58-encoded Solana public key")


class SolanaChallengeResponse(BaseModel):
    nonce: str = Field(description="Opaque identifier for this challenge; send it back with the signature")
    message: str = Field(description="The exact message the wallet must sign, byte-for-byte")
    expires_at: datetime


class SolanaVerifyRequest(BaseModel):
    wallet_address: str = Field(description="Base58-encoded Solana public key")
    nonce: str = Field(description="The nonce returned by /auth/solana/challenge")
    signature: str = Field(description="Base58-encoded signature over the exact challenge message")


class OAuthExchangeRequest(BaseModel):
    code: str = Field(description="One-time code received on the desktop app's loopback callback")
