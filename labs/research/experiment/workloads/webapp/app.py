"""CPU-bound HTTP service whose load is set by the request rate."""

import hashlib
import os

from flask import Flask

ROUNDS = int(os.environ.get("CPU_ROUNDS", "200000"))

app = Flask(__name__)


@app.get("/")
def index():
    return "croft-webapp\n"


@app.get("/health")
def health():
    return "ok\n"


# cpu burns a fixed amount of CPU per request, so throughput is bounded by the pod's CPU limit.
@app.get("/cpu")
def cpu():
    digest = b"croft"
    for _ in range(ROUNDS):
        digest = hashlib.sha256(digest).digest()
    return digest.hex()[:16] + "\n"
