def fetch(key):
    return {"key": key}


class Client:
    @classmethod
    def create(cls):
        return cls()
