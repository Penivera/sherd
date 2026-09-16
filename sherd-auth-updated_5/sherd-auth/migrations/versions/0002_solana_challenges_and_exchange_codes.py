"""solana challenges and oauth exchange codes

Revision ID: 0002
Revises: 0001
Create Date: 2026-09-11

"""
from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

revision = "0002"
down_revision = "0001"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table(
        "solana_challenges",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("wallet_address", sa.String(length=64), nullable=False),
        sa.Column("nonce", sa.String(length=64), nullable=False),
        sa.Column("message", sa.String(length=512), nullable=False),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("expires_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("consumed_at", sa.DateTime(timezone=True), nullable=True),
    )
    op.create_index(
        "ix_solana_challenges_wallet_address", "solana_challenges", ["wallet_address"]
    )
    op.create_unique_constraint(
        "uq_solana_challenges_nonce", "solana_challenges", ["nonce"]
    )
    op.create_index("ix_solana_challenges_nonce", "solana_challenges", ["nonce"])

    op.create_table(
        "oauth_exchange_codes",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("code_hash", sa.String(length=64), nullable=False),
        sa.Column(
            "user_id",
            postgresql.UUID(as_uuid=True),
            sa.ForeignKey("users.id", ondelete="CASCADE"),
            nullable=False,
        ),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("expires_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("consumed_at", sa.DateTime(timezone=True), nullable=True),
    )
    op.create_unique_constraint(
        "uq_oauth_exchange_codes_code_hash", "oauth_exchange_codes", ["code_hash"]
    )
    op.create_index(
        "ix_oauth_exchange_codes_code_hash", "oauth_exchange_codes", ["code_hash"]
    )


def downgrade() -> None:
    op.drop_index("ix_oauth_exchange_codes_code_hash", table_name="oauth_exchange_codes")
    op.drop_table("oauth_exchange_codes")
    op.drop_index("ix_solana_challenges_nonce", table_name="solana_challenges")
    op.drop_index("ix_solana_challenges_wallet_address", table_name="solana_challenges")
    op.drop_table("solana_challenges")
