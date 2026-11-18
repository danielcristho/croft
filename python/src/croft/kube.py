"""Container names from Kubernetes, read through kubectl, to label dataset rows that carry a container_id only."""

import json
import subprocess

Names = dict[str, tuple[str, str, str]]


def names(kubeconfig: str | None = None, context: str | None = None) -> Names:
    """container_id to (namespace, pod, container) for every pod the API server holds, with each container's
    previous instance; containers of deleted pods are absent."""
    command = ["kubectl", "get", "pods", "--all-namespaces", "-o", "json"]
    command += ["--kubeconfig", kubeconfig] if kubeconfig else []
    command += ["--context", context] if context else []
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    if result.returncode:
        raise RuntimeError(f"kubectl: {result.stderr.strip()}")
    return names_of(json.loads(result.stdout))


def names_of(pods: dict) -> Names:
    found: Names = {}
    for pod in pods.get("items", []):
        namespace, name = pod["metadata"]["namespace"], pod["metadata"]["name"]
        status = pod.get("status", {})
        for container in status.get("initContainerStatuses", []) + status.get("containerStatuses", []):
            previous = container.get("lastState", {}).get("terminated", {})
            for identity in (container.get("containerID"), previous.get("containerID")):
                if identity:
                    found[identity.split("://")[-1]] = (namespace, name, container["name"])
    return found
