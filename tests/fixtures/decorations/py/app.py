from fastapi import FastAPI

app = FastAPI()


@app.get("/x")
def index():
    return helper()


def helper():
    return 1
