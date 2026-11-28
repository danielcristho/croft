"""An application of apps.json under Locust or open-loop load, with Prometheus series for the same window."""

import json
import os
import re
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

from dataclasses import asdict

from . import kube, linkerd, prometheus, records, rps, scaler
from .apps import ROOT, App
from .profiles import PROFILES, Profile

# Consts
LOCUSTFILE = ROOT / "load" / "locustfile.py"
READY_CHECKS = 3
READY_TIMEOUT_S = 120
WARMUP_RPS = 5
PROBES = {
    "default": None,
    "relaxed": {"livenessProbe": None, "readinessProbe": {"timeoutSeconds": 10, "failureThreshold": 6}},
    "none": {"livenessProbe": None, "readinessProbe": None},
}


# deploy restarts the Deployment at one replica with a new HPA, so no scale-down window or backlog carries over.
def deploy(app: App, replicas: int | None, node: str | None = None, probes: str = "default", hpa: bool = True, mesh: bool = False,
           also: tuple[str, ...] = ()) -> None:
    check_image(app)
    targets = app.scaled(also)
    for target in targets:
        check_finished_pods(target)
    kube.apply((app.manifests / "k8s.yaml").read_text())
    linkerd.set_mesh(app, mesh)
    for target in targets:
        prepare(target, replicas, node, probes, hpa)
    wait_for_ingress(app)

# prepare pins one scaled Deployment, restarts it at its replicas and, for an HPA run, gives it a new HPA.
def prepare(app: App, replicas: int | None, node: str | None, probes: str = "default", hpa: bool = True) -> None:
    pin(app, node)
    if PROBES[probes]:
        patch = json.dumps({"spec": {"template": {"spec": {"containers": [{"name": app.container, **PROBES[probes]}]}}}})
        kube.kubectl("patch", f"deployment/{app.deployment}", "-n", app.namespace, "--type=strategic", "-p", patch)
    kube.kubectl("delete", "hpa", app.deployment, "-n", app.namespace, "--ignore-not-found")
    kube.kubectl("scale", f"deployment/{app.deployment}", "-n", app.namespace, f"--replicas={replicas or 1}")
    kube.kubectl("rollout", "restart", f"deployment/{app.deployment}", "-n", app.namespace)
    kube.kubectl("rollout", "status", f"deployment/{app.deployment}", "-n", app.namespace, "--timeout=300s")
    if replicas is None and hpa:
        kube.apply(app.hpa_manifest.read_text())

# pin sets or clears the node selector. apply leaves fields outside the manifest alone, so a pin outlives the run that set it.
def pin(app: App, node: str | None) -> None:
    selector = {"kubernetes.io/hostname": node} if node else None
    patch = json.dumps({"spec": {"template": {"spec": {"nodeSelector": selector}}}})
    kube.kubectl("patch", f"deployment/{app.deployment}", "-n", app.namespace, "--type=merge", "-p", patch)


# check_image stops before a run, since a node without the image leaves every pod there hanging in ImagePullBackOff.
def check_image(app: App) -> None:
    image = re.search(r"image:\s*(\S+)", (app.manifests / "k8s.yaml").read_text()).group(1)
    missing, unknown = kube.nodes_without(image)
    if unknown:
        print(f"{image}: not among the {kube.REPORTED_IMAGES} images {', '.join(unknown)} report; assuming it is loaded")
    if missing:
        raise SystemExit(
            f"{image} is missing on {', '.join(missing)}; load it with labs/k3s-cluster/load-image.yml -e image={image}"
        )


# check_finished_pods stops before a run, since the HPA counts a Succeeded pod as a replica at 0% CPU and never scales up.
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


# wait_for_ingress waits past pod readiness, since the ingress controller routes to a new backend a few seconds later.
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


# locust runs with --exit-code-on-error 0, since failed requests are data, not a reason to stop the run.
def locust(app: App, users: int, seconds: int, csv_prefix: Path | None, env: dict[str, str] | None = None) -> None:
    command = [
        "locust", "-f", str(LOCUSTFILE), "--host", app.host, "--headless",
        "-u", str(users), "-r", "5", "-t", f"{seconds}s", "--exit-code-on-error", "0", "--only-summary",
    ]
    if csv_prefix:
        command += ["--csv", str(csv_prefix), "--csv-full-history"]
    subprocess.run(command, check=True, env={**os.environ, "APP_PATH": app.path, **(env or {})}, capture_output=csv_prefix is None)


def rescales(app: App, since_ms: int, kind: str = "HorizontalPodAutoscaler", reason: str = "SuccessfulRescale") -> list[dict]:
    """Scaling events since a run started, the HPA's decisions, or the Deployment's replica set changes from any scaler.
    Both objects carry the Deployment's name, so another scaled Deployment's events stay out."""
    selector = f"involvedObject.kind={kind},involvedObject.name={app.deployment}"
    events = kube.get("events", app.namespace, "--field-selector", selector)["items"]
    found = []
    for event in events:
        if event.get("reason") != reason or not event.get("lastTimestamp"):
            continue
        last_ms = kube.epoch_ms(event["lastTimestamp"])
        if last_ms >= since_ms:
            first_ms = kube.epoch_ms(event.get("firstTimestamp") or event["lastTimestamp"])
            found.append({"first_ms": first_ms, "last_ms": last_ms, "count": event.get("count", 1), "message": event["message"]})
    return sorted(found, key=lambda rescale: rescale["first_ms"])


# pods records each app pod's creation and last Ready time since the run's deploy, to the second.
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


# export_series writes the app's Prometheus series, services and collector signals for the window, plus the call graph
# with a mesh, and returns the mesh name for the run record.
def export_series(app: App, directory: Path, start_ms: int, end_ms: int, mesh: bool) -> str | None:
    start_s, end_s = start_ms // 1000, end_ms // 1000
    prometheus.export(directory / "prometheus.csv", app.namespace, app.deployment, start_s, end_s)
    prometheus.export_services(directory / "services.csv", app, start_s, end_s)
    prometheus.export_signals(directory / "signals.csv", app.namespace, start_s, end_s)
    if not mesh:
        return None
    linkerd.export_edges(directory / "edges.csv", app.namespace, app.deployment, start_s, end_s)
    return "linkerd"


# merge_pods adds the pods only Prometheus saw, ones a scale-down deleted before the API server stopped listing them,
# with their node and container ids from kube-state-metrics.
def merge_pods(live: list[dict], recorded: dict[str, dict], since_ms: int) -> list[dict]:
    found = {pod["pod"]: dict(pod) for pod in live}
    for name, seen in recorded.items():
        if name in found:
            if found[name]["ready_ms"] is None:
                found[name]["ready_ms"] = seen.get("ready_ms")
        elif seen.get("created_ms") is not None and seen["created_ms"] >= since_ms:
            found[name] = {"pod": name, "node": seen.get("node"), "created_ms": seen["created_ms"],
                           "ready_ms": seen.get("ready_ms"), "container_ids": list(seen.get("container_ids", []))}
    return sorted(found.values(), key=lambda pod: (pod["created_ms"], pod["pod"]))


# recorded_pods is every pod of the app's Deployment Prometheus saw since since_ms, with when it was created and
# first Ready, its node and its container ids.
def recorded_pods(app: App, since_ms: int, until_ms: int | None = None) -> dict[str, dict]:
    start_s, end_s = since_ms // 1000, (until_ms or records.now_ms()) // 1000 + 1
    found = prometheus.pod_times(app.namespace, app.deployment, start_s, end_s)
    for name, pod in prometheus.pod_containers(app.namespace, app.deployment, start_s, end_s).items():
        found.setdefault(name, {}).update(pod)
    return found


# run_containers maps the container ids of pods created since the deploy to their node, since a scale-down deletes
# them mid-run before kube.containers lists them, and croft rows join on container ids.
def run_containers(recorded: dict[str, dict], since_ms: int) -> dict[str, str | None]:
    return {
        container_id: pod.get("node")
        for pod in recorded.values()
        if pod.get("created_ms") is not None and pod["created_ms"] >= since_ms
        for container_id in pod.get("container_ids", [])
    }

# live_containers maps the container ids of every scaled Deployment's running pods to their node.
def live_containers(targets: list[App]) -> dict[str, str]:
    found = {}
    for target in targets:
        found |= kube.containers(target.namespace, "-l", target.selector)
    return found

def timeline(app: App, start_ms: int, deployed_ms: int, recorded: dict[str, dict]) -> dict:
    return {
        "rescales": rescales(app, start_ms),
        "replica_sets": rescales(app, start_ms, "Deployment", "ScalingReplicaSet"),
        "pods": merge_pods(pods(app, deployed_ms), recorded, deployed_ms),
    }


# backfill_pods adds pods a finished run's timeline.json missed while Prometheus still holds them, using the
# earliest pod already there as the run's deploy time.
def backfill_pods(app: App, record: dict) -> tuple[int, int]:
    path = records.DATA / "web" / record["run_id"] / "timeline.json"
    found = json.loads(path.read_text())
    before = found.get("pods", [])
    since_ms = min([pod["created_ms"] for pod in before] + [record["started_ms"]])
    found["pods"] = merge_pods(before, recorded_pods(app, since_ms, record["ended_ms"]), since_ms)
    path.write_text(json.dumps(found, indent=1))
    return len(before), len(found["pods"])


def run_level(app: App, name: str, users: int, warmup: int, seconds: int, replicas: int | None, steps: str | None, step_seconds: int, deployed_ms: int, shape: str = "step", node: str | None = None, mesh: bool = False, concurrent: bool = False) -> dict:
    directory = records.DATA / "web" / name
    directory.mkdir(parents=True, exist_ok=True)
    if warmup:
        locust(app, users, warmup, None)
    containers = kube.containers(app.namespace, "-l", app.selector)
    start_ms = records.now_ms()
    env = {"LOAD_STEPS": steps, "STEP_SECONDS": str(step_seconds), "LOAD_SHAPE": shape} if steps else None
    locust(app, users, seconds, directory / "locust", env)
    end_ms = records.now_ms()
    containers |= kube.containers(app.namespace, "-l", app.selector)
    recorded = recorded_pods(app, deployed_ms)
    containers = {**run_containers(recorded, deployed_ms), **containers}
    recorded_mesh = export_series(app, directory, start_ms, end_ms, mesh)
    (directory / "timeline.json").write_text(json.dumps(timeline(app, start_ms, deployed_ms, recorded), indent=1))
    return {
        "run_id": name,
        "app": app.name,
        "users": users,
        "host": app.host,
        "endpoints": [asdict(endpoint) for endpoint in app.endpoints()],
        "mix": False,
        "steps": steps,
        "shape": shape if steps else None,
        "replicas": replicas,
        "node": node,
        "autoscaling": "hpa" if replicas is None else "static",
        "mesh": recorded_mesh,
        "concurrent": concurrent,
        "collector": scaler.collector(),
        "started_ms": start_ms,
        "ended_ms": end_ms,
        "containers": containers,
        "directory": str(directory.relative_to(ROOT)),
    }


def run(app: App, prefix: str, levels: list[int], warmup: int, seconds: int, cooldown: int, replicas: int | None, steps: str | None, step_seconds: int, shape: str = "step", node: str | None = None, mesh: bool = False, concurrent: bool = False) -> None:
    deployed_ms = records.now_ms()
    deploy(app, replicas, node, mesh=mesh)
    if steps:
        suffix = "steps" if shape == "step" else shape
        plan = [(f"{prefix}-{suffix}", max(int(users) for users in steps.split(",")))]
        seconds = len(steps.split(",")) * step_seconds
    else:
        plan = [(f"{prefix}-u{users}", users) for users in levels]
    for i, (name, users) in enumerate(plan, 1):
        record = run_level(app, name, users, 0 if steps else warmup, seconds, replicas, steps, step_seconds, deployed_ms, shape, node, mesh, concurrent)
        records.append("web", record)
        print(f"[{i}/{len(plan)}] {name}: {json.dumps({k: record[k] for k in ('users', 'autoscaling', 'replicas')})}")
        time.sleep(cooldown)


def run_profile(app: App, prefix: str, name: str, scale: float, warmup: int, cooldown: int, replicas: int | None, node: str | None = None, probes: str = "default", arm: str = "none", mesh: bool = False, concurrent: bool = False, mix: bool = False, also: tuple[str, ...] = ()) -> None:
    """One open-loop run of a cloud2023 load profile against the app's request path or its endpoint mix,
    with croft scale replacing the HPA when asked."""
    endpoints = app.endpoints(mix)
    # with_scaler = arm == "croft"
    with_scaler = arm != "none"
    targets = app.scaled(also)
    if with_scaler:
        for target in targets:
            scaler.check(target)
    deployed_ms = records.now_ms()
    deploy(app, replicas, node, probes, hpa=not with_scaler, mesh=mesh, also=also)
    profile = PROFILES[name].scaled(scale)
    run_id = f"{prefix}-{name}"
    directory = records.DATA / "web" / run_id
    directory.mkdir(parents=True, exist_ok=True)
    try:
        if with_scaler:
            for target in targets:
                scaler.start(target, arm)
        if warmup:
            rps.run(app.host, endpoints, Profile("constant", warmup, rps=WARMUP_RPS), None, run=run_id, users=app.users(mix))
        containers = live_containers(targets)
        start_ms = records.now_ms()
        rps.run(app.host, endpoints, profile, directory / "workload.csv", directory / "endpoints.csv", run=run_id,
                users=app.users(mix))
        end_ms = records.now_ms()
    finally:
        # A failed run keeps the scaler's log too, since it shows what croft decided before the failure.
        # Every scaler stops even when a log cannot be read, since one left running would scale the next run.
        if with_scaler:
            try:
                for target in targets:
                    scaler.save_log(target, directory / f"scaler{target.suffix}.log")
            finally:
                for target in targets:
                    scaler.stop(target)
    containers |= live_containers(targets)
    recorded = {target.deployment: recorded_pods(target, deployed_ms) for target in targets}
    for pods_seen in recorded.values():
        containers = {**run_containers(pods_seen, deployed_ms), **containers}
    recorded_mesh = export_series(app, directory, start_ms, end_ms, mesh)
    for target in targets:
        path = directory / f"timeline{target.suffix}.json"
        path.write_text(json.dumps(timeline(target, start_ms, deployed_ms, recorded[target.deployment]), indent=1))
    record = {
        "run_id": run_id,
        "app": app.name,
        "load": "open-loop",
        "host": app.host,
        "endpoints": [asdict(endpoint) for endpoint in endpoints],
        "mix": mix,
        "users": app.users(mix),
        "generator": rps.GENERATOR,
        "profile": asdict(profile),
        "scale": scale,
        "replicas": replicas,
        "node": node,
        "autoscaling": "static" if replicas is not None else "croft" if with_scaler else "hpa",
        "probes": probes,
        "scaler": None if arm == "none" else arm,
        "targets": [target.deployment for target in targets],
        "mesh": recorded_mesh,
        "concurrent": concurrent,
        "collector": scaler.collector(),
        "started_ms": start_ms,
        "ended_ms": end_ms,
        "containers": containers,
        "directory": str(directory.relative_to(ROOT)),
    }
    records.append("web", record)
    print(f"{run_id}: {app.name} {profile.name} x{scale:g} for {profile.duration_s:g} s, {record['autoscaling']}{' + croft' if with_scaler else ''}")
    time.sleep(cooldown)
