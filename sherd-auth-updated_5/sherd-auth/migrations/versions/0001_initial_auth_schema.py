"""initial auth schema

Revision ID: 0001
Revises:
Create Date: 2026-09-11

"""
from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

revision = "0001"
down_revision = None
branch_labels = None
depends_on = None

auth_provider_enum = postgresql.ENUM(
    "email", "google", "github", "solana", name="authprovider", create_type=False
)


def upgrade() -> None:
    auth_provider_enum.create(op.get_bind(), checkfirst=True)

    op.create_table(
        "users",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("email", sa.String(length=320), nullable=True),
        sa.Column("password_hash", sa.String(length=255), nullable=True),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("updated_at", sa.DateTime(timezone=True), nullable=False),
    )
    op.create_unique_constraint("uq_users_email", "users", ["email"])

    op.create_table(
        "auth_identities",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column(
            "user_id",
            postgresql.UUID(as_uuid=True),
            sa.ForeignKey("users.id", ondelete="CASCADE"),
            nullable=False,
        ),
        sa.Column("provider", auth_provider_enum, nullable=False),
        sa.Column("provider_account_id", sa.String(length=255), nullable=False),
        sa.Column("provider_metadata", sa.String(length=500), nullable=True),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.UniqueConstraint(
            "provider", "provider_account_id", name="uq_provider_account"
        ),
    )
    op.create_index(
        "ix_auth_identities_user_id", "auth_identities", ["user_id"]
    )


def downgrade() -> None:
    op.drop_index("ix_auth_identities_user_id", table_name="auth_identities")
    op.drop_table("auth_identities")
    op.drop_constraint("uq_users_email", "users", type_="unique")
    op.drop_table("users")
    auth_provider_enum.drop(op.get_bind(), checkfirst=True)
