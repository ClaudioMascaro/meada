from flask import Blueprint

accounts = Blueprint("accounts", __name__, url_prefix="/accounts")


@accounts.route("/<name>", methods=["PUT"])
def update(name):
    return name
