"""Run records, one JSON line per run, and the locks that keep experiments apart: the whole cluster, or one app with --concurrent."""

import fcntl
import json
import sys
import time
from pathlib import Path

DATA = Path(__file__).resolve().parent.parent / "data"
LOCK = DATA / "experiment.lock"


def now_ms() -> int:
    return int(time.time() * 1000)


def hold_lock(app: str | None = None) -> list:
    """The whole cluster by default; with app, a share of it that runs of other apps may hold at the same time."""
    DATA.mkdir(parents=True, exist_ok=True)
    handles = [LOCK.open("w")]
    try:
        fcntl.flock(handles[0], (fcntl.LOCK_SH if app else fcntl.LOCK_EX) | fcntl.LOCK_NB)
    except BlockingIOError:
        handles[0].close()
        sys.exit(f"another experiment holds {LOCK}; wait for it or stop it first"
                 if not app else f"an experiment holds the whole cluster ({LOCK}); --concurrent runs wait for it")
    if app:
        handles.append((DATA / f"experiment-{app}.lock").open("w"))
        try:
            fcntl.flock(handles[1], fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            for handle in handles:
                handle.close()
            sys.exit(f"another run of {app} holds {DATA / f'experiment-{app}.lock'}")
    return handles


def append(suite: str, record: dict) -> Path:
    path = DATA / suite / "runs.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as runs:
        # Concurrent runs of several apps append to the same file.
        fcntl.flock(runs, fcntl.LOCK_EX)
        runs.write(json.dumps(record) + "\n")
    return path
