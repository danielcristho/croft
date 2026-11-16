"""Runs the croft experiments on the kubernetes cluster: GPU workloads as Jobs, the apps of apps.json under load."""

import argparse

from experiment import calibrate_scaler, compare, gpu, records, web
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
    web_args.add_argument("--app", choices=sorted(APPS), default="hotel-reservation", help="application under test, from apps.json")
    web_args.add_argument("--prefix", required=True, help="run name prefix, e.g. r10 or hpa")
    web_args.add_argument("--users", nargs="+", type=int, default=[10, 20, 40, 80])
    web_args.add_argument("--replicas", type=int, help="fixed replica count; omit to let the HPA scale")
    web_args.add_argument("--steps", help="one stepped run instead, e.g. 10,20,40,80")
    web_args.add_argument("--step-seconds", type=int, default=120)
    web_args.add_argument("--node", help="pin every replica to one node")
    web_args.add_argument("--shape", choices=["step", "ramp"], default="step", help="hold each step, or ramp between them")
    web_args.add_argument("--profile", choices=sorted(PROFILES), help="one open-loop run of a cloud2023 RPS profile instead")
    web_args.add_argument("--scale", type=float, help="multiply every rate of --profile; default the app's scale in apps.json")
    web_args.add_argument("--probes", choices=sorted(web.PROBES), default="default", help="relaxed: no liveness restart, slower readiness; none: no probes, the HPA best case")
    web_args.add_argument("--scaler", choices=["none", "croft"], default="none", help="croft: croft scale replaces the HPA; needs --profile")
    web_args.add_argument("--concurrent", action="store_true", help="share the cluster with runs of other apps; the run records it")
    web_args.add_argument("--mesh", action="store_true", help="Linkerd on the app's namespace; records the call graph in edges.csv")
    web_args.add_argument("--warmup", type=int, help="seconds; default 60, or 30 with --profile.")
    web_args.add_argument("--seconds", type=int, default=300)
    web_args.add_argument("--cooldown", type=int, help="seconds; default 30, or 15 with --profile.")

    scaler_args = suites.add_parser("calibrate-scaler", help="syscalls and CPU per request of one app on one pod, for croft scale")
    scaler_args.add_argument("--app", choices=sorted(APPS), required=True)
    scaler_args.add_argument("--levels", nargs="+", type=int, default=[20, 50, 100], help="constant RPS levels, all below one pod's saturation")
    scaler_args.add_argument("--seconds", type=int, default=75, help="seconds per level; the first 20 settle")
    scaler_args.add_argument("--node", help="pin the pod to one node")

    compare_args = suites.add_parser("compare", help="score burst runs per arm, the HPA alone against the HPA with croft scale")
    compare_args.add_argument("--prefix", nargs="+", default=[], help="run id prefixes to score; default every burst run")

    args = parser.parse_args()
    lock = None
    if args.suite == "compare":
        compare.run(args.prefix)
    elif args.suite == "gpu":
        lock = None if args.dry_run else records.hold_lock()
        gpu.run(args.only, args.reps, args.seconds, args.cooldown, args.seed, args.node, args.dry_run)
    elif args.suite == "calibrate-scaler":
        lock = records.hold_lock()
        calibrate_scaler.run(APPS[args.app], args.levels, args.seconds, args.node)
    else:
        if args.scaler != "none" and (not args.profile or args.replicas):
            parser.error(f"--scaler {args.scaler} needs --profile and an autoscaler, so no --replicas")
        # croft scale reads the first container's CPU request and counts every container's syscalls, the proxy's too.
        if args.mesh and args.scaler == "croft":
            parser.error("--mesh does not work with --scaler croft: croft scale would count the linkerd-proxy's syscalls")
        if args.concurrent and args.scaler == "croft":
            parser.error("--concurrent does not work with --scaler croft: the croft arm needs the cluster alone")
        lock = records.hold_lock(args.app if args.concurrent else None)
        app = APPS[args.app]
        if args.profile:
            warmup = 30 if args.warmup is None else args.warmup
            cooldown = 15 if args.cooldown is None else args.cooldown
            scale = app.scale if args.scale is None else args.scale
            web.run_profile(app, args.prefix, args.profile, scale, warmup, cooldown, args.replicas, args.node, args.probes, args.scaler, args.mesh, args.concurrent)
        else:
            warmup = 60 if args.warmup is None else args.warmup
            cooldown = 30 if args.cooldown is None else args.cooldown
            web.run(app, args.prefix, args.users, warmup, args.seconds, cooldown, args.replicas, args.steps, args.step_seconds, args.shape, args.node, args.mesh, args.concurrent)
    for handle in lock or []:
        handle.close()


if __name__ == "__main__":
    main()
