from flask import Blueprint

pages = Blueprint("pages", __name__)


@pages.route("/home")
def home():
    return "home"
