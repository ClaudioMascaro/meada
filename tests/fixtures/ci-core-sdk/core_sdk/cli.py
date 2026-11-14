import os
import sys

import requests

BASE = os.environ["CORE_API_URL"]


def publish_main():
    return requests.post(f"{BASE}/v1/packages", json={"name": sys.argv[1]})


def health_main():
    return requests.get(f"{BASE}/v1/health")


def main():
    return health_main()
