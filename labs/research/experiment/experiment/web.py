"""The webapp under Locust load, one run per user count, with Prometheus series for the same window."""

import json
import os
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

from . import kube, prometheus, records

# Consts
NAMESPACE = "croft-exp"
DEPLOYMENT = "webapp"
ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = ROOT / "workloads" / "webapp"
LOCUSTFILE = ROOT / "load" / "locustfile.py"
APP_HOST = os.environ.get("APP_HOST", "http://web.127.0.0.1.sslip.io:30080")
READY_CHECKS = 3
READY_TIMEOUT_S = 120


def deploy(replicas: int | None) -> None:
    kube.apply((MANIFESTS / "k8s.yaml").read_text())
    if replicas is None:
        kube.apply((MANIFESTS / "hpa.yaml").read_text())
    else:
        kube.kubectl("delete", "hpa", DEPLOYMENT, "-n", NAMESPACE, "--ignore-not-found")
        kube.kubectl("scale", f"deployment/{DEPLOYMENT}", "-n", NAMESPACE, f"--replicas={replicas}")
    kube.kubectl("rollout", "status", f"deployment/{DEPLOYMENT}", "-n", NAMESPACE, "--timeout=300s")
    wait_for_ingress()


# wait_for_ingress waits past pod readiness: the ingress controller routes to a new backend a few seconds later.
def wait_for_ingress() -> None:
    passed, deadline = 0, time.time() + READY_TIMEOUT_S
    while passed < READY_CHECKS:
        if time.time() > deadline:
            raise TimeoutError(f"{APP_HOST}/health did not answer 200 within {READY_TIMEOUT_S} s")
        try:
            with urllib.request.urlopen(f"{APP_HOST}/health", timeout=5) as response:
                passed = passed + 1 if response.status == 200 else 0
        except (urllib.error.URLError, TimeoutError):
            passed = 0
        time.sleep(1)


# locust runs with --exit-code-on-error 0: failed requests are data, not a reason to stop the run.
def locust(users: int, seconds: int, csv_prefix: Path | None, env: dict[str, str] | None = None) -> None:
    command = [
        "locust", "-f", str(LOCUSTFILE), "--host", APP_HOST, "--headless",
        "-u", str(users), "-r", "5", "-t", f"{seconds}s", "--exit-code-on-error", "0", "--only-summary",
    ]
    if csv_prefix:
        command += ["--csv", str(csv_prefix), "--csv-full-history"]
    subprocess.run(command, check=True, env={**os.environ, **(env or {})}, capture_output=csv_prefix is None)


def run_level(name: str, users: int, warmup: int, seconds: int, replicas: int | None, steps: str | None, step_seconds: int) -> dict:
    directory = records.DATA / "web" / name
    directory.mkdir(parents=True, exist_ok=True)
    if warmup:
        locust(users, warmup, None)
    start_ms = records.now_ms()
    env = {"LOAD_STEPS": steps, "STEP_SECONDS": str(step_seconds)} if steps else None
    locust(users, seconds, directory / "locust", env)
    end_ms = records.now_ms()
    prometheus.export(directory / "prometheus.csv", NAMESPACE, DEPLOYMENT, start_ms // 1000, end_ms // 1000)
    return {
        "run_id": name,
        "users": users,
        "steps": steps,
        "replicas": replicas,
        "autoscaling": "hpa" if replicas is None else "static",
        "started_ms": start_ms,
        "ended_ms": end_ms,
        "directory": str(directory.relative_to(ROOT)),
    }


def run(prefix: str, levels: list[int], warmup: int, seconds: int, cooldown: int, replicas: int | None, steps: str | None, step_seconds: int) -> None:
    deploy(replicas)
    if steps:
        # One run that climbs through every step; the shape in the locustfile sets the users.
        plan = [(f"{prefix}-steps", max(int(users) for users in steps.split(",")))]
        seconds = len(steps.split(",")) * step_seconds
    else:
        plan = [(f"{prefix}-u{users}", users) for users in levels]
    for i, (name, users) in enumerate(plan, 1):
        record = run_level(name, users, 0 if steps else warmup, seconds, replicas, steps, step_seconds)
        records.append("web", record)
        print(f"[{i}/{len(plan)}] {name}: {json.dumps({k: record[k] for k in ('users', 'autoscaling', 'replicas')})}")
        time.sleep(cooldown)
