"""The webapp under Locust load, one run per user count, with Prometheus series for the same window."""

import json
import os
import re
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


# deploy starts an HPA run from one replica and a new HPA, so no scale-down window carries over from the last run.
def deploy(replicas: int | None, node: str | None = None) -> None:
    check_image()
    kube.apply((MANIFESTS / "k8s.yaml").read_text())
    pin(node)
    kube.kubectl("delete", "hpa", DEPLOYMENT, "-n", NAMESPACE, "--ignore-not-found")
    kube.kubectl("scale", f"deployment/{DEPLOYMENT}", "-n", NAMESPACE, f"--replicas={replicas or 1}")
    kube.kubectl("rollout", "status", f"deployment/{DEPLOYMENT}", "-n", NAMESPACE, "--timeout=300s")
    if replicas is None:
        kube.apply((MANIFESTS / "hpa.yaml").read_text())
    wait_for_ingress()


# pin sets or clears the node selector; apply leaves fields outside the manifest alone, so a pin outlives the run that set it.
def pin(node: str | None) -> None:
    selector = {"kubernetes.io/hostname": node} if node else None
    patch = json.dumps({"spec": {"template": {"spec": {"nodeSelector": selector}}}})
    kube.kubectl("patch", f"deployment/{DEPLOYMENT}", "-n", NAMESPACE, "--type=merge", "-p", patch)


# check_image stops before a run: a node without the image makes every pod scheduled there hang in ImagePullBackOff.
def check_image() -> None:
    image = re.search(r"image:\s*(\S+)", (MANIFESTS / "k8s.yaml").read_text()).group(1)
    missing = kube.nodes_without(image)
    if missing:
        raise SystemExit(
            f"{image} is missing on {', '.join(missing)}; load it with labs/k3s-cluster/load-image.yml -e image={image}"
        )


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


def rescales(since_ms: int) -> list[dict]:
    """HPA scaling decisions since a run started, from the namespace's events."""
    events = kube.get("events", NAMESPACE, "--field-selector", "involvedObject.kind=HorizontalPodAutoscaler")["items"]
    found = []
    for event in events:
        if event.get("reason") != "SuccessfulRescale" or not event.get("lastTimestamp"):
            continue
        last_ms = kube.epoch_ms(event["lastTimestamp"])
        if last_ms >= since_ms:
            first_ms = kube.epoch_ms(event.get("firstTimestamp") or event["lastTimestamp"])
            found.append({"first_ms": first_ms, "last_ms": last_ms, "count": event.get("count", 1), "message": event["message"]})
    return sorted(found, key=lambda rescale: rescale["first_ms"])


# pods records when each webapp pod was created and last turned Ready, to the second.
def pods() -> list[dict]:
    found = []
    for pod in kube.get("pods", NAMESPACE, "-l", f"app={DEPLOYMENT}")["items"]:
        ready = next((c for c in pod["status"].get("conditions", []) if c["type"] == "Ready"), {})
        found.append({
            "pod": pod["metadata"]["name"],
            "node": pod["spec"].get("nodeName"),
            "created_ms": kube.epoch_ms(pod["metadata"]["creationTimestamp"]),
            "ready_ms": kube.epoch_ms(ready["lastTransitionTime"]) if ready.get("status") == "True" else None,
            "container_ids": [kube.container_id(status) for status in pod["status"].get("containerStatuses", [])],
        })
    return found


def run_level(name: str, users: int, warmup: int, seconds: int, replicas: int | None, steps: str | None, step_seconds: int, shape: str = "step", node: str | None = None) -> dict:
    directory = records.DATA / "web" / name
    directory.mkdir(parents=True, exist_ok=True)
    if warmup:
        locust(users, warmup, None)
    # Containers are read at both ends: the HPA adds pods during a run, and croft rows join on their ids.
    containers = kube.containers(NAMESPACE, "-l", f"app={DEPLOYMENT}")
    start_ms = records.now_ms()
    env = {"LOAD_STEPS": steps, "STEP_SECONDS": str(step_seconds), "LOAD_SHAPE": shape} if steps else None
    locust(users, seconds, directory / "locust", env)
    end_ms = records.now_ms()
    containers |= kube.containers(NAMESPACE, "-l", f"app={DEPLOYMENT}")
    prometheus.export(directory / "prometheus.csv", NAMESPACE, DEPLOYMENT, start_ms // 1000, end_ms // 1000)
    timeline = {"rescales": rescales(start_ms), "pods": pods()}
    (directory / "timeline.json").write_text(json.dumps(timeline, indent=1))
    return {
        "run_id": name,
        "users": users,
        "steps": steps,
        "shape": shape if steps else None,
        "replicas": replicas,
        "node": node,
        "autoscaling": "hpa" if replicas is None else "static",
        "started_ms": start_ms,
        "ended_ms": end_ms,
        "containers": containers,
        "directory": str(directory.relative_to(ROOT)),
    }


def run(prefix: str, levels: list[int], warmup: int, seconds: int, cooldown: int, replicas: int | None, steps: str | None, step_seconds: int, shape: str = "step", node: str | None = None) -> None:
    deploy(replicas, node)
    if steps:
        # One run that climbs through every step; the shape in the locustfile sets the users.
        suffix = "steps" if shape == "step" else shape
        plan = [(f"{prefix}-{suffix}", max(int(users) for users in steps.split(",")))]
        seconds = len(steps.split(",")) * step_seconds
    else:
        plan = [(f"{prefix}-u{users}", users) for users in levels]
    for i, (name, users) in enumerate(plan, 1):
        record = run_level(name, users, 0 if steps else warmup, seconds, replicas, steps, step_seconds, shape, node)
        records.append("web", record)
        print(f"[{i}/{len(plan)}] {name}: {json.dumps({k: record[k] for k in ('users', 'autoscaling', 'replicas')})}")
        time.sleep(cooldown)
