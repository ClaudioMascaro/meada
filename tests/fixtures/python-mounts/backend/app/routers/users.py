from fastapi import APIRouter

router = APIRouter(prefix="/users")


@router.get("/{user_id}")
def get_user(user_id: int):
    return user_id
