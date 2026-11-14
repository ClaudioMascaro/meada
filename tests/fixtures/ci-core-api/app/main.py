from fastapi import FastAPI

app = FastAPI()


@app.post("/v1/packages")
def register_package():
    return {"registered": True}


@app.get("/v1/health")
def service_health():
    return {"ok": True}
