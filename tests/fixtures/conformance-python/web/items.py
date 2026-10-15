from fastapi import APIRouter

router = APIRouter(prefix="/catalog", tags=["items"])


@router.get("/{item_id}")
async def read_item(item_id: int):
    return item_id


@router.post("")
def create_item():
    return None
