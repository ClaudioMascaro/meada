"""A Flask app whose Blueprint is imported through a module that does not resolve."""

from flask import Flask

from app.views import pages

site = Flask(__name__)


@site.route("/about")
def about():
    return "about"


site.register_blueprint(pages)
