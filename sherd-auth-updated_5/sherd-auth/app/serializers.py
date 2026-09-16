from app.models import User
from app.schemas import UserOut


def to_user_out(user: User) -> UserOut:
    return UserOut(
        id=user.id,
        email=user.email,
        providers=[identity.provider.value for identity in user.identities],
    )
