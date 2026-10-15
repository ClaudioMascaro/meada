"""A FastAPI app under `backend/`: `app.routers` does not resolve from the repository root, so
Meada cannot see where the users router is mounted."""

from fastapi import FastAPI
from plugins import wrap

from app.routers import users

app = FastAPI()
v1 = FastAPI()
legacy = FastAPI()


@app.get("/health")
def health():
    return "ok"


@v1.get("/ping")
def ping():
    return "pong"


@legacy.get("/old")
def old():
    return "old"


app.include_router(users.router, prefix="/api")
# A sub-application, mounted under its path.
app.mount("/v1", v1)
# A mount of something Meada cannot place: any other app may be under it.
app.mount("/legacy", wrap(legacy))
