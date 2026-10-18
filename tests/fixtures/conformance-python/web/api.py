"""A FastAPI app: routers with nested prefixes, one router under two prefixes."""

from fastapi import APIRouter, FastAPI

from . import items
from .factory import build_admin, mount, relay

API = "/api"

app = FastAPI()


@app.get("/health")
def health():
    return {"ok": True}


@app.api_route("/ping", methods=["GET", "post"])
def ping():
    return "pong"


@app.api_route("/echo")
def echo():
    return "echo"


v1 = APIRouter(prefix=API + "/v1")
v1.include_router(items.router, prefix="/items")
app.include_router(v1)
app.include_router(items.router, prefix="/legacy")
app.include_router(relay(items.router, 2), prefix="/relay")
mount(app, build_admin())
