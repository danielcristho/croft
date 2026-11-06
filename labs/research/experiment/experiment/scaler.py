"""croft scale next to the HPA for one run: deployed from deploy/kubernetes/scaler.yaml with the app's calibration."""

import json
from pathlib import Path

from . import kube
from .apps import App

MANIFEST = Path(__file__).resolve().parents[4] / "deploy" / "kubernetes" / "scaler.yaml"
NAMESPACE = "croft"
COLLECTORS = "croft-collector"
METRICS_ENV = "CROFT_METRICS_ADDR"
READY_TIMEOUT_S = 120
# The scale module logs its rate and replica estimate at debug on every poll; compare reads them from scaler.log.
LOG_LEVEL = "info,croft::command::scale=debug"


def name(app: App) -> str:
    return f"croft-scaler-{app.name}"


# cpu_cores reads a Kubernetes CPU quantity: "200m", "1" or "1.5".
def cpu_cores(quantity: str) -> float:
    return float(quantity[:-1]) / 1000 if quantity.endswith("m") else float(quantity)


# limit_cores reads the app container's CPU limit from the deployed Deployment.
def limit_cores(app: App) -> float:
    deployment = kube.get("deployment", app.namespace, app.deployment)
    container = next(c for c in deployment["spec"]["template"]["spec"]["containers"] if c["name"] == app.container)
    return cpu_cores(container["resources"]["limits"]["cpu"])


# check stops before a run: without a calibration or collector metrics the scaler would run blind.
def check(app: App) -> None:
    if app.scaler is None:
        raise SystemExit(f"apps.json has no scaler calibration for {app.name}")
    daemonset = kube.get("daemonset", NAMESPACE, COLLECTORS)
    env = daemonset["spec"]["template"]["spec"]["containers"][0].get("env", [])
    if not any(var["name"] == METRICS_ENV and var.get("value") for var in env):
        raise SystemExit(f"{COLLECTORS} does not set {METRICS_ENV}; deploy the collector with deploy/kubernetes/collector.yaml")


# documents splits kubectl's output, which prints one JSON object per manifest document back to back.
def documents(text: str) -> list[dict]:
    decoder, found, position = json.JSONDecoder(), [], 0
    while position < len(text.rstrip()):
        document, position = decoder.raw_decode(text, position)
        found.append(document)
        while position < len(text) and text[position].isspace():
            position += 1
    return found


def objects(app: App) -> list[dict]:
    """The manifest's objects, with its Deployment renamed and pointed at the app."""
    items = documents(kube.kubectl("create", "--dry-run=client", "-o", "json", "-f", str(MANIFEST)))
    return [retarget(item, app) if item["kind"] == "Deployment" else item for item in items]


# retarget points the manifest's scaler Deployment at the app, with its calibration and the run's log level.
def retarget(deployment: dict, app: App) -> dict:
    labels = {"app": "croft-scaler", "target": app.name}
    deployment["metadata"]["name"] = name(app)
    deployment["metadata"]["labels"] = labels
    deployment["spec"]["selector"]["matchLabels"] = labels
    deployment["spec"]["template"]["metadata"]["labels"] = labels
    scaler = next(c for c in deployment["spec"]["template"]["spec"]["containers"] if c["name"] == "scaler")
    scaler["args"] = [
        "scale",
        f"--namespace={app.namespace}",
        f"--deployment={app.deployment}",
        f"--syscall={app.scaler.syscall}",
        f"--calls-per-request={app.scaler.calls_per_request}",
        f"--cpu-per-request-ms={app.scaler.cpu_per_request_ms}",
    ]
    env = [var for var in scaler.get("env", []) if var["name"] != "RUST_LOG"]
    scaler["env"] = [*env, {"name": "RUST_LOG", "value": LOG_LEVEL}]
    return deployment


# start replaces any scaler left from an earlier run, so its confirmation streak starts empty.
def start(app: App) -> None:
    stop(app)
    for item in objects(app):
        kube.apply(json.dumps(item))
    kube.kubectl("rollout", "status", f"deployment/{name(app)}", "-n", NAMESPACE, f"--timeout={READY_TIMEOUT_S}s")


def save_log(app: App, path: Path) -> None:
    path.write_text(kube.kubectl("logs", f"deployment/{name(app)}", "-n", NAMESPACE, "-c", "scaler", "--timestamps"))


def stop(app: App) -> None:
    kube.kubectl("delete", "deployment", name(app), "-n", NAMESPACE, "--ignore-not-found", "--wait=true")
