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


def epoch_ms(timestamp: str) -> int:
    return int(datetime.fromisoformat(timestamp.replace("Z", "+00:00")).timestamp() * 1000)
