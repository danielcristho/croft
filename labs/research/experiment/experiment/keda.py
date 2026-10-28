"""KEDA in place of the HPA for one run: a ScaledObject on the ingress request rate, at the capacity croft scale assumes."""

import json
import subprocess

from . import kube
from .apps import App

# Consts
PROMETHEUS = "http://monitoring-kube-prometheus-prometheus.monitoring.svc:9090"
CRD = "scaledobjects.keda.sh"
# The HPA's target and bound, so the three arms differ only in their signal.
TARGET = 0.7
MAX_REPLICAS = 10
POLLING_S = 5
RATE_WINDOW = "30s"


# cpu_cores reads a Kubernetes CPU quantity: "200m", "1" or "1.5".
def cpu_cores(quantity: str) -> float:
    return float(quantity[:-1]) / 1000 if quantity.endswith("m") else float(quantity)


# threshold is the requests per second one pod takes at the target, from the app's CPU per request.
def threshold(app: App, limit_cores: float) -> float:
    return round(TARGET * limit_cores * 1000 / app.scaler.cpu_per_request_ms, 1)


# query counts the requests the ingress answered for the app's Service; nginx records each one when it completes.
def query(app: App) -> str:
    return f'sum(rate(nginx_ingress_controller_requests{{namespace="{app.namespace}",service="{app.deployment}"}}[{RATE_WINDOW}]))'


def scaled_object(app: App, limit_cores: float) -> dict:
    return {
        "apiVersion": "keda.sh/v1alpha1",
        "kind": "ScaledObject",
        "metadata": {"name": app.deployment, "namespace": app.namespace},
        "spec": {
            "scaleTargetRef": {"name": app.deployment},
            "minReplicaCount": 1,
            "maxReplicaCount": MAX_REPLICAS,
            "pollingInterval": POLLING_S,
            "triggers": [{
                "type": "prometheus",
                "metadata": {"serverAddress": PROMETHEUS, "query": query(app), "threshold": str(threshold(app, limit_cores))},
            }],
        },
    }


# check stops before a run: the threshold needs the app's CPU per request, and the cluster needs KEDA.
def check(app: App) -> None:
    if app.scaler is None:
        raise SystemExit(f"apps.json has no calibration for {app.name}; KEDA's threshold comes from its CPU per request")
    try:
        kube.kubectl("get", "crd", CRD)
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"KEDA is not installed ({CRD} is missing); see RUNNING.md") from error


# limit_cores reads the app container's CPU limit from the deployed Deployment.
def limit_cores(app: App) -> float:
    deployment = kube.get("deployment", app.namespace, app.deployment)
    container = next(c for c in deployment["spec"]["template"]["spec"]["containers"] if c["name"] == app.container)
    return cpu_cores(container["resources"]["limits"]["cpu"])


def start(app: App) -> None:
    kube.apply(json.dumps(scaled_object(app, limit_cores(app))))
    kube.wait("condition=Ready", f"scaledobject/{app.deployment}", app.namespace, 60)


# stop removes the ScaledObject and with it the HPA KEDA made; without KEDA there is nothing to remove.
def stop(app: App) -> None:
    try:
        kube.kubectl("delete", "scaledobject", app.deployment, "-n", app.namespace, "--ignore-not-found", "--wait=true")
    except subprocess.CalledProcessError as error:
        if "the server doesn't have a resource type" not in error.stderr:
            raise
