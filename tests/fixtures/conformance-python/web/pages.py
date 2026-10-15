"""A Flask app with Blueprints: a prefix from the constructor, one replaced on registration."""

from flask import Blueprint, Flask

from .accounts import accounts

app = Flask(__name__)
reports = Blueprint("reports", __name__, url_prefix="/reports")


@app.route("/")
def index():
    return "home"


@app.route("/orders/<int:order_id>", methods=["GET", "DELETE"])
def order(order_id):
    return str(order_id)


@reports.route("/daily/<day>")
def daily(day):
    return day


@app.get("/shortcut")
def shortcut():
    """A Flask shortcut: not a route Meada reads."""
    return ""


def fetch(url):
    """Named like the JavaScript global, but a function of this repository."""
    return url


fetch("/health")
app.register_blueprint(reports)
app.register_blueprint(accounts, url_prefix="/members")
