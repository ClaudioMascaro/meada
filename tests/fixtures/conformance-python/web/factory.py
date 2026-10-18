"""Routers passed as arguments and returned from functions."""

from fastapi import APIRouter


def build_admin():
    admin = APIRouter(prefix="/admin")

    @admin.delete("/users/{user_id}")
    def remove_user(user_id: str):
        return user_id

    return admin


def mount(target, router):
    target.include_router(router, prefix="/internal")


def relay(router, depth):
    if depth:
        return relay(router, depth - 1)
    return router
