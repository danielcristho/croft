"""Run records, one JSON line per run, and the lock that keeps two experiments off the cluster at once."""

import fcntl
import json
import sys
import time
from pathlib import Path

DATA = Path(__file__).resolve().parent.parent / "data"
LOCK = DATA / "experiment.lock"


def now_ms() -> int:
    return int(time.time() * 1000)


def hold_lock():
    DATA.mkdir(parents=True, exist_ok=True)
    handle = LOCK.open("w")
    try:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(f"another experiment holds {LOCK}; wait for it or stop it first")
    return handle


def append(suite: str, record: dict) -> Path:
    path = DATA / suite / "runs.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as runs:
        runs.write(json.dumps(record) + "\n")
    return path
