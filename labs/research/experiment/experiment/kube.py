"""kubectl wrappers; the cluster comes from KUBECONFIG."""

import json
import subprocess
from datetime import datetime


def kubectl(*args: str, stdin: str | None = None) -> str:
    return subprocess.run(["kubectl", *args], input=stdin, check=True, capture_output=True, text=True).stdout


def apply(manifest: str) -> None:
    kubectl("apply", "-f", "-", stdin=manifest)


def get(kind: str, namespace: str, *selector: str) -> dict:
    return json.loads(kubectl("get", kind, "-n", namespace, *selector, "-o", "json"))


def wait(condition: str, target: str, namespace: str, timeout_s: int) -> bool:
    command = ["kubectl", "wait", f"--for={condition}", target, "-n", namespace, f"--timeout={timeout_s}s"]
    return subprocess.run(command, capture_output=True).returncode == 0


# container_id strips the runtime scheme, matching the id croft reads from the cgroup path.
def container_id(status: dict) -> str:
    return status.get("containerID", "").partition("://")[2]


def containers(namespace: str, *selector: str) -> dict[str, str]:
    """Maps each started container of the selected pods to its node."""
    return {
        container_id(status): pod["spec"].get("nodeName")
        for pod in get("pods", namespace, *selector)["items"]
        for status in pod["status"].get("containerStatuses", [])
        if status.get("containerID")
    }


# The kubelet reports at most this many images per node, largest first (nodeStatusMaxImages).
REPORTED_IMAGES = 50


# nodes_without reads each node's image list. A full list cannot show a small image, so such a node is unknown, not missing.
def nodes_without(image: str, nodes: list[dict] | None = None) -> tuple[list[str], list[str]]:
    """Ready nodes whose runtime does not hold the image, and Ready nodes whose full image list cannot tell."""
    missing, unknown = [], []
    for node in nodes if nodes is not None else json.loads(kubectl("get", "nodes", "-o", "json"))["items"]:
        ready = any(c["type"] == "Ready" and c["status"] == "True" for c in node["status"]["conditions"])
        images = node["status"].get("images", [])
        names = {name for entry in images for name in entry.get("names") or []}
        if not ready or any(name == image or name.endswith("/" + image) for name in names):
            continue
        (unknown if len(images) >= REPORTED_IMAGES else missing).append(node["metadata"]["name"])
    return missing, unknown


def ready_nodes() -> list[str]:
    return sorted(
        node["metadata"]["name"]
        for node in json.loads(kubectl("get", "nodes", "-o", "json"))["items"]
        if any(c["type"] == "Ready" and c["status"] == "True" for c in node["status"]["conditions"])
    )


def epoch_ms(timestamp: str) -> int:
    return int(datetime.fromisoformat(timestamp.replace("Z", "+00:00")).timestamp() * 1000)
