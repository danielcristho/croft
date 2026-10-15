"""GPU workloads with known labels, each run as a Kubernetes Job that asks for one GPU."""

import json
import random
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path

from . import kube, records

# Consts
NAMESPACE = "croft-exp"
CONFIGMAP = "croft-gpu-workloads"
TORCH_IMAGE = "pytorch/pytorch:2.14.0-cuda13.0-cudnn9-runtime"
OLLAMA_IMAGE = "ollama/ollama:0.34.4"
WORKLOADS = Path(__file__).resolve().parent.parent / "workloads" / "gpu"
STARTUP_ALLOWANCE_S = 900

def torch_command(label: str, seconds: float, params: dict) -> list[str]:
    return ["python", "/workloads/workloads.py", label, "--seconds", str(seconds), "--params", json.dumps(params)]


def ollama_command(_label: str, seconds: float, params: dict) -> list[str]:
    return ["/bin/sh", "/workloads/ollama.sh", params["model"], str(seconds), str(params["think_ms"])]


@dataclass(frozen=True)
class Scenario:
    variants: list[dict]
    image: str = TORCH_IMAGE
    command: object = torch_command
    volumes: list[dict] = field(default_factory=list)
    mounts: list[dict] = field(default_factory=list)


OLLAMA_CACHE = {"name": "ollama", "hostPath": {"path": "/var/lib/croft/ollama", "type": "DirectoryOrCreate"}}

# Each repetition takes the next variant, so a label covers several shapes of the same workload.
SCENARIOS = {
    "train-cnn": Scenario([{"width": 32, "batch": 64}, {"width": 64, "batch": 128}, {"width": 96, "batch": 256}]),
    "train-transformer": Scenario([
        {"width": 256, "layers": 4, "batch": 16},
        {"width": 512, "layers": 6, "batch": 8},
        {"width": 384, "layers": 8, "batch": 16},
    ]),
    "infer-batch-cnn": Scenario([{"width": 32, "batch": 256}, {"width": 64, "batch": 512}, {"width": 96, "batch": 1024}]),
    "infer-generate-lm": Scenario([
        {"width": 256, "layers": 4, "batch": 1, "new_tokens": 64},
        {"width": 512, "layers": 6, "batch": 4, "new_tokens": 128},
        {"width": 384, "layers": 8, "batch": 8, "new_tokens": 32},
    ]),
    "serve-http": Scenario([
        {"width": 32, "batch": 1, "think_ms": 50},
        {"width": 64, "batch": 8, "think_ms": 10},
        {"width": 96, "batch": 32, "think_ms": 200},
    ]),
    "serve-llm-ollama": Scenario(
        [
            {"model": "qwen2.5:0.5b", "think_ms": 100},
            {"model": "llama3.2:1b", "think_ms": 500},
            {"model": "gemma3:1b", "think_ms": 0},
        ],
        image=OLLAMA_IMAGE,
        command=ollama_command,
        volumes=[OLLAMA_CACHE],
        mounts=[{"name": "ollama", "mountPath": "/root/.ollama"}],
    ),
    "gpu-idle": Scenario([{"width": 1024}, {"width": 4096}, {"width": 8192}]),
}

def plan(labels: list[str], reps: int, seed: int) -> list[tuple[str, dict]]:
    order = random.Random(seed)
    runs = []
    for rep in range(reps):
        batch = [(label, SCENARIOS[label].variants[rep % len(SCENARIOS[label].variants)]) for label in labels]
        # Shuffled per repetition so no label always sits at the same time of day.
        order.shuffle(batch)
        runs.extend(batch)
    return runs


def publish_workloads() -> None:
    manifest = kube.kubectl(
        "create", "configmap", CONFIGMAP, "-n", NAMESPACE, "--dry-run=client", "-o", "yaml",
        *(f"--from-file={path}" for path in sorted(WORKLOADS.iterdir()) if path.is_file()),
    )
    kube.apply(manifest)


def job_manifest(name: str, label: str, run_id: str, params: dict, seconds: float, node: str | None) -> str:
    scenario = SCENARIOS[label]
    spec = {
        "runtimeClassName": "nvidia",
        "restartPolicy": "Never",
        "containers": [{
            "name": "workload",
            "image": scenario.image,
            "command": scenario.command(label, seconds, params),
            "resources": {"limits": {"nvidia.com/gpu": 1}},
            "env": [{"name": "NVIDIA_DRIVER_CAPABILITIES", "value": "compute,utility"}],
            "volumeMounts": [{"name": "workloads", "mountPath": "/workloads"}, *scenario.mounts],
        }],
        "volumes": [{"name": "workloads", "configMap": {"name": CONFIGMAP, "defaultMode": 0o755}}, *scenario.volumes],
    }
    if node:
        spec["nodeSelector"] = {"kubernetes.io/hostname": node}
    labels = {"croft.scenario": label, "croft.run": run_id}
    job = {
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {"name": name, "namespace": NAMESPACE, "labels": labels},
        "spec": {
            "backoffLimit": 0,
            "activeDeadlineSeconds": int(seconds) + STARTUP_ALLOWANCE_S,
            "template": {"metadata": {"labels": labels}, "spec": spec},
        },
    }
    return json.dumps(job)

def run_one(label: str, params: dict, seconds: float, node: str | None) -> dict:
    run_id = uuid.uuid4().hex[:12]
    name = f"gpu-{label}-{run_id}"
    kube.apply(job_manifest(name, label, run_id, params, seconds, node))
    succeeded = kube.wait("condition=complete", f"job/{name}", NAMESPACE, int(seconds) + STARTUP_ALLOWANCE_S)
    pod = kube.get("pods", NAMESPACE, "-l", f"job-name={name}")["items"][0]
    container = pod["status"]["containerStatuses"][0]
    terminated = container.get("state", {}).get("terminated", {})
    if not succeeded:
        print(kube.kubectl("logs", pod["metadata"]["name"], "-n", NAMESPACE, "--tail", "20"))
    kube.kubectl("delete", "job", name, "-n", NAMESPACE, "--wait=true", "--cascade=foreground")
    return {
        "run_id": run_id,
        "label": label,
        "params": params,
        "image": SCENARIOS[label].image,
        "node": pod["spec"].get("nodeName"),
        "pod": pod["metadata"]["name"],
        "container_id": kube.container_id(container),
        "started_ms": kube.epoch_ms(terminated["startedAt"]) if "startedAt" in terminated else None,
        "ended_ms": kube.epoch_ms(terminated["finishedAt"]) if "finishedAt" in terminated else records.now_ms(),
        "succeeded": succeeded,
    }


def run(labels: list[str], reps: int, seconds: float, cooldown: float, seed: int, node: str | None, dry_run: bool) -> None:
    runs = plan(labels, reps, seed)
    print(f"{len(runs)} GPU runs, about {len(runs) * (seconds + cooldown) / 60:.0f} min plus image pulls")
    if dry_run:
        for label, params in runs:
            print(f"  {label:<18} {json.dumps(params)}")
        return
    kube.apply(json.dumps({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": NAMESPACE}}))
    publish_workloads()
    for i, (label, params) in enumerate(runs, 1):
        record = run_one(label, params, seconds, node)
        records.append("gpu", record)
        status = "ok" if record["succeeded"] else "FAILED"
        print(f"[{i}/{len(runs)}] {label:<18} {status}  node={record['node']}  {record['container_id'][:12]}")
        time.sleep(cooldown)
