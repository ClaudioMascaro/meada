"""Prefixes and paths with a part Meada cannot read: they yield no route, not a guessed one."""

import settings
from fastapi import APIRouter, FastAPI

app = FastAPI()
users = APIRouter(prefix=f"{settings.API_PREFIX}/users")


@users.get("/{user_id}")
def read_user(user_id: int):
    return user_id


@app.get("/partial/{part_id}")
def read_part(part_id: int):
    return part_id


def build(name):
    router = APIRouter()

    @router.get(f"/{name}/items")
    def items():
        return []

    return router


app.include_router(users)
app.include_router(build("shop"))
