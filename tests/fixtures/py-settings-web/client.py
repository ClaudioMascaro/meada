import requests
from typing import ClassVar

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="APP_")
    api_url: str = ""
    orders: str = Field("", alias="orders_url")
    required: str
    kept: ClassVar[str] = "http://other.example.com"
    both: str = Field("", alias="IGNORED", validation_alias="BOTH_URL")


class Plain:
    api_url = ""


settings = Settings()
plain = Plain()


def via_prefix():
    return requests.get(f"{settings.api_url}/orders")


def via_alias():
    return requests.get(f"{settings.orders}/orders")


def not_settings():
    return requests.get(f"{plain.api_url}/orders")


def no_default():
    return requests.get(f"{settings.required}/orders")


def validation_alias():
    return requests.get(f"{settings.both}/orders")


def class_var():
    return requests.get(f"{settings.kept}/orders")
