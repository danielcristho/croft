"""The webapp under Locust load, one run per user count, with Prometheus series for the same window."""

import json
import os
import re
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

from dataclasses import asdict

from . import kube, prometheus, records, rps, scaler
from .apps import ROOT, App
from .profiles import PROFILES, Profile

# Consts
LOCUSTFILE = ROOT / "load" / "locustfile.py"
READY_CHECKS = 3
READY_TIMEOUT_S = 120
# cloud2023 warms up at 5 RPS for 30 s and cools down for 15 s.
WARMUP_RPS = 5
# relaxed keeps an overloaded pod alive and in its Service: no liveness restart, readiness tolerates 60 s of slow answers.
PROBES = {
    "default": None,
    "relaxed": {"livenessProbe": None, "readinessProbe": {"timeoutSeconds": 10, "failureThreshold": 6}},
    "none": {"livenessProbe": None, "readinessProbe": None},
}


# deploy starts an HPA run from one fresh replica and a new HPA, so no scale-down window or request backlog carries over.
def deploy(app: App, replicas: int | None, node: str | None = None, probes: str = "default") -> None:
    check_image(app)
    check_finished_pods(app)
    kube.apply((app.manifests / "k8s.yaml").read_text())
    pin(app, node)
    if PROBES[probes]:
        patch = json.dumps({"spec": {"template": {"spec": {"containers": [{"name": app.container, **PROBES[probes]}]}}}})
        kube.kubectl("patch", f"deployment/{app.deployment}", "-n", app.namespace, "--type=strategic", "-p", patch)
    kube.kubectl("delete", "hpa", app.deployment, "-n", app.namespace, "--ignore-not-found")
    kube.kubectl("scale", f"deployment/{app.deployment}", "-n", app.namespace, f"--replicas={replicas or 1}")
    # Without probes nothing restarts an overloaded pod; a restart drops the requests queued by the last run.
    kube.kubectl("rollout", "restart", f"deployment/{app.deployment}", "-n", app.namespace)
    kube.kubectl("rollout", "status", f"deployment/{app.deployment}", "-n", app.namespace, "--timeout=300s")
    if replicas is None:
        kube.apply((app.manifests / "hpa.yaml").read_text())
    wait_for_ingress(app)


# pin sets or clears the node selector; apply leaves fields outside the manifest alone, so a pin outlives the run that set it.
def pin(app: App, node: str | None) -> None:
    selector = {"kubernetes.io/hostname": node} if node else None
    patch = json.dumps({"spec": {"template": {"spec": {"nodeSelector": selector}}}})
    kube.kubectl("patch", f"deployment/{app.deployment}", "-n", app.namespace, "--type=merge", "-p", patch)


# check_image stops before a run: a node without the image makes every pod scheduled there hang in ImagePullBackOff.
def check_image(app: App) -> None:
    image = re.search(r"image:\s*(\S+)", (app.manifests / "k8s.yaml").read_text()).group(1)
    missing = kube.nodes_without(image)
    if missing:
        raise SystemExit(
            f"{image} is missing on {', '.join(missing)}; load it with labs/k3s-cluster/load-image.yml -e image={image}"
        )


# check_finished_pods stops before a run: the HPA counts a Succeeded pod as a replica with 0% CPU and then never scales up.
def check_finished_pods(app: App) -> None:
    finished = [
        pod["metadata"]["name"]
        for pod in kube.get("pods", app.namespace, "-l", app.selector)["items"]
        if pod["status"].get("phase") in ("Succeeded", "Failed")
    ]
    if finished:
        raise SystemExit(
            f"{len(finished)} finished {app.deployment} pods in {app.namespace} distort the HPA; delete them with "
            f"kubectl delete pods -n {app.namespace} -l {app.selector} --field-selector=status.phase!=Running"
        )


# wait_for_ingress waits past pod readiness: the ingress controller routes to a new backend a few seconds later.
def wait_for_ingress(app: App) -> None:
    health = f"{app.host}{app.health}"
    passed, deadline = 0, time.time() + READY_TIMEOUT_S
    while passed < READY_CHECKS:
        if time.time() > deadline:
            raise TimeoutError(f"{health} did not answer 200 within {READY_TIMEOUT_S} s")
        try:
            with urllib.request.urlopen(health, timeout=5) as response:
                passed = passed + 1 if response.status == 200 else 0
        except (urllib.error.URLError, TimeoutError):
            passed = 0
        time.sleep(1)


# locust runs with --exit-code-on-error 0: failed requests are data, not a reason to stop the run.
def locust(app: App, users: int, seconds: int, csv_prefix: Path | None, env: dict[str, str] | None = None) -> None:
    command = [
        "locust", "-f", str(LOCUSTFILE), "--host", app.host, "--headless",
        "-u", str(users), "-r", "5", "-t", f"{seconds}s", "--exit-code-on-error", "0", "--only-summary",
    ]
    if csv_prefix:
        command += ["--csv", str(csv_prefix), "--csv-full-history"]
    subprocess.run(command, check=True, env={**os.environ, **(env or {})}, capture_output=csv_prefix is None)


def rescales(app: App, since_ms: int, kind: str = "HorizontalPodAutoscaler", reason: str = "SuccessfulRescale") -> list[dict]:
    """Scaling events since a run started: the HPA's decisions, or the Deployment's replica set changes from any scaler."""
    events = kube.get("events", app.namespace, "--field-selector", f"involvedObject.kind={kind}")["items"]
    found = []
    for event in events:
        if event.get("reason") != reason or not event.get("lastTimestamp"):
            continue
        last_ms = kube.epoch_ms(event["lastTimestamp"])
        if last_ms >= since_ms:
            first_ms = kube.epoch_ms(event.get("firstTimestamp") or event["lastTimestamp"])
            found.append({"first_ms": first_ms, "last_ms": last_ms, "count": event.get("count", 1), "message": event["message"]})
    return sorted(found, key=lambda rescale: rescale["first_ms"])


# pods records when each app pod created since the run's deploy was created and last turned Ready, to the second.
def pods(app: App, since_ms: int) -> list[dict]:
    found = []
    for pod in kube.get("pods", app.namespace, "-l", app.selector)["items"]:
        if kube.epoch_ms(pod["metadata"]["creationTimestamp"]) < since_ms:
            continue
        ready = next((c for c in pod["status"].get("conditions", []) if c["type"] == "Ready"), {})
        found.append({
            "pod": pod["metadata"]["name"],
            "node": pod["spec"].get("nodeName"),
            "created_ms": kube.epoch_ms(pod["metadata"]["creationTimestamp"]),
            "ready_ms": kube.epoch_ms(ready["lastTransitionTime"]) if ready.get("status") == "True" else None,
            "container_ids": [kube.container_id(status) for status in pod["status"].get("containerStatuses", [])],
        })
    return found


def timeline(app: App, start_ms: int, deployed_ms: int) -> dict:
    return {
        "rescales": rescales(app, start_ms),
        "replica_sets": rescales(app, start_ms, "Deployment", "ScalingReplicaSet"),
        "pods": pods(app, deployed_ms),
    }


def run_level(app: App, name: str, users: int, warmup: int, seconds: int, replicas: int | None, steps: str | None, step_seconds: int, deployed_ms: int, shape: str = "step", node: str | None = None) -> dict:
    directory = records.DATA / "web" / name
    directory.mkdir(parents=True, exist_ok=True)
    if warmup:
        locust(app, users, warmup, None)
    # Containers are read at both ends: the HPA adds pods during a run, and croft rows join on their ids.
    containers = kube.containers(app.namespace, "-l", app.selector)
    start_ms = records.now_ms()
    env = {"LOAD_STEPS": steps, "STEP_SECONDS": str(step_seconds), "LOAD_SHAPE": shape} if steps else None
    locust(app, users, seconds, directory / "locust", env)
    end_ms = records.now_ms()
    containers |= kube.containers(app.namespace, "-l", app.selector)
    prometheus.export(directory / "prometheus.csv", app.namespace, app.deployment, start_ms // 1000, end_ms // 1000)
    (directory / "timeline.json").write_text(json.dumps(timeline(app, start_ms, deployed_ms), indent=1))
    return {
        "run_id": name,
        "app": app.name,
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


def run(app: App, prefix: str, levels: list[int], warmup: int, seconds: int, cooldown: int, replicas: int | None, steps: str | None, step_seconds: int, shape: str = "step", node: str | None = None) -> None:
    deployed_ms = records.now_ms()
    deploy(app, replicas, node)
    if steps:
        # One run that climbs through every step; the shape in the locustfile sets the users.
        suffix = "steps" if shape == "step" else shape
        plan = [(f"{prefix}-{suffix}", max(int(users) for users in steps.split(",")))]
        seconds = len(steps.split(",")) * step_seconds
    else:
        plan = [(f"{prefix}-u{users}", users) for users in levels]
    for i, (name, users) in enumerate(plan, 1):
        record = run_level(app, name, users, 0 if steps else warmup, seconds, replicas, steps, step_seconds, deployed_ms, shape, node)
        records.append("web", record)
        print(f"[{i}/{len(plan)}] {name}: {json.dumps({k: record[k] for k in ('users', 'autoscaling', 'replicas')})}")
        time.sleep(cooldown)


def run_profile(app: App, prefix: str, name: str, scale: float, warmup: int, cooldown: int, replicas: int | None, node: str | None = None, probes: str = "default", with_scaler: bool = False) -> None:
    """One open-loop run of a cloud2023 load profile against the app's request path; croft scale joins the HPA when asked."""
    if with_scaler:
        scaler.check(app)
    deployed_ms = records.now_ms()
    deploy(app, replicas, node, probes)
    profile = PROFILES[name].scaled(scale)
    run_id = f"{prefix}-{name}"
    directory = records.DATA / "web" / run_id
    directory.mkdir(parents=True, exist_ok=True)
    try:
        if with_scaler:
            scaler.start(app)
        if warmup:
            rps.run(app.url, Profile("constant", warmup, rps=WARMUP_RPS), None)
        containers = kube.containers(app.namespace, "-l", app.selector)
        start_ms = records.now_ms()
        rps.run(app.url, profile, directory / "workload.csv")
        end_ms = records.now_ms()
    finally:
        # A failed run keeps the scaler's log too: it shows what croft decided before the failure.
        if with_scaler:
            try:
                scaler.save_log(app, directory / "scaler.log")
            finally:
                scaler.stop(app)
    containers |= kube.containers(app.namespace, "-l", app.selector)
    prometheus.export(directory / "prometheus.csv", app.namespace, app.deployment, start_ms // 1000, end_ms // 1000)
    (directory / "timeline.json").write_text(json.dumps(timeline(app, start_ms, deployed_ms), indent=1))
    record = {
        "run_id": run_id,
        "app": app.name,
        "load": "open-loop",
        "profile": asdict(profile),
        "scale": scale,
        "replicas": replicas,
        "node": node,
        "autoscaling": "hpa" if replicas is None else "static",
        "probes": probes,
        "scaler": "croft" if with_scaler else None,
        "started_ms": start_ms,
        "ended_ms": end_ms,
        "containers": containers,
        "directory": str(directory.relative_to(ROOT)),
    }
    records.append("web", record)
    print(f"{run_id}: {app.name} {profile.name} x{scale:g} for {profile.duration_s:g} s, {record['autoscaling']}{' + croft' if with_scaler else ''}")
    time.sleep(cooldown)
