"""Runs the croft experiments on the kubernetes cluster: GPU workloads as Jobs, the apps of apps.json under load."""

import argparse
from pathlib import Path

from experiment import calibrate, compare, gpu, records, web
from experiment.apps import APPS
from experiment.profiles import PROFILES


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    suites = parser.add_subparsers(dest="suite", required=True)

    gpu_args = suites.add_parser("gpu", help="labelled GPU workloads, one Job at a time")
    gpu_args.add_argument("--only", nargs="+", choices=gpu.SCENARIOS, default=list(gpu.SCENARIOS))
    gpu_args.add_argument("--reps", type=int, default=3)
    gpu_args.add_argument("--seconds", type=float, default=90)
    gpu_args.add_argument("--cooldown", type=float, default=10)
    gpu_args.add_argument("--seed", type=int, default=0)
    gpu_args.add_argument("--node", help="pin every run to one node")
    gpu_args.add_argument("--dry-run", action="store_true")

    web_args = suites.add_parser("web", help="an app of apps.json under Locust or open-loop load")
    web_args.add_argument("--app", choices=sorted(APPS), default="webapp", help="application under test, from apps.json")
    web_args.add_argument("--prefix", required=True, help="run name prefix, e.g. r10 or hpa")
    web_args.add_argument("--users", nargs="+", type=int, default=[10, 20, 40, 80])
    web_args.add_argument("--replicas", type=int, help="fixed replica count; omit to let the HPA scale")
    web_args.add_argument("--steps", help="one stepped run instead, e.g. 10,20,40,80")
    web_args.add_argument("--step-seconds", type=int, default=120)
    web_args.add_argument("--node", help="pin every replica to one node")
    web_args.add_argument("--shape", choices=["step", "ramp"], default="step", help="hold each step, or ramp between them")
    web_args.add_argument("--profile", choices=sorted(PROFILES), help="one open-loop run of a cloud2023 RPS profile instead")
    web_args.add_argument("--scale", type=float, default=1.0, help="multiply every rate of --profile")
    web_args.add_argument("--probes", choices=sorted(web.PROBES), default="default", help="relaxed: no liveness restart, slower readiness; none: no probes, the HPA best case")
    web_args.add_argument("--scaler", choices=["none", "croft"], default="none", help="croft: croft scale runs next to the HPA; needs --profile")
    web_args.add_argument("--warmup", type=int, help="seconds; default 60, or 30 with --profile.")
    web_args.add_argument("--seconds", type=int, default=300)
    web_args.add_argument("--cooldown", type=int, help="seconds; default 30, or 15 with --profile.")

    calibrate_args = suites.add_parser("calibrate", help="node speed: CPU per request with one pinned replica per node")
    calibrate_args.add_argument("--prefix", default="speed", help="run name prefix; runs are <prefix>-<node>-u<users>")
    calibrate_args.add_argument("--nodes", nargs="+", help="nodes to measure; default every Ready node")
    calibrate_args.add_argument("--users", type=int, default=1)
    calibrate_args.add_argument("--warmup", type=int, default=30)
    calibrate_args.add_argument("--seconds", type=int, default=120)
    calibrate_args.add_argument("--cooldown", type=int, default=10)
    calibrate_args.add_argument("--croft-data", type=Path, default=calibrate.DEFAULT_CROFT_DATA, help="per-node croft datasets")
    calibrate_args.add_argument("--from-runs", action="store_true", help="compute from existing runs, measure nothing")

    compare_args = suites.add_parser("compare", help="score burst runs per arm, the HPA alone against the HPA with croft scale")
    compare_args.add_argument("--prefix", nargs="+", default=[], help="run id prefixes to score; default every burst run")

    args = parser.parse_args()
    lock = None
    if args.suite == "compare":
        compare.run(args.prefix)
    elif args.suite == "gpu":
        lock = None if args.dry_run else records.hold_lock()
        gpu.run(args.only, args.reps, args.seconds, args.cooldown, args.seed, args.node, args.dry_run)
    elif args.suite == "calibrate":
        lock = None if args.from_runs else records.hold_lock()
        calibrate.run(args.prefix, args.nodes, args.users, args.warmup, args.seconds, args.cooldown, args.croft_data, args.from_runs)
    else:
        if args.scaler == "croft" and (not args.profile or args.replicas):
            parser.error("--scaler croft needs --profile and the HPA, so no --replicas")
        lock = records.hold_lock()
        app = APPS[args.app]
        if args.profile:
            warmup = 30 if args.warmup is None else args.warmup
            cooldown = 15 if args.cooldown is None else args.cooldown
            web.run_profile(app, args.prefix, args.profile, args.scale, warmup, cooldown, args.replicas, args.node, args.probes, args.scaler == "croft")
        else:
            warmup = 60 if args.warmup is None else args.warmup
            cooldown = 30 if args.cooldown is None else args.cooldown
            web.run(app, args.prefix, args.users, warmup, args.seconds, cooldown, args.replicas, args.steps, args.step_seconds, args.shape, args.node)
    if lock:
        lock.close()


if __name__ == "__main__":
    main()
