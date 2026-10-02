"""Runs the croft experiments on the k3s cluster: GPU workloads as Jobs, the webapp under Locust load."""

import argparse

from experiment import gpu, records, web


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

    web_args = suites.add_parser("web", help="the webapp under Locust load")
    web_args.add_argument("--prefix", required=True, help="run name prefix, e.g. r10 or hpa")
    web_args.add_argument("--users", nargs="+", type=int, default=[10, 20, 40, 80])
    web_args.add_argument("--replicas", type=int, help="fixed replica count; omit to let the HPA scale")
    web_args.add_argument("--steps", help="one stepped run instead, e.g. 10,20,40,80")
    web_args.add_argument("--step-seconds", type=int, default=120)
    web_args.add_argument("--warmup", type=int, default=60)
    web_args.add_argument("--seconds", type=int, default=300)
    web_args.add_argument("--cooldown", type=int, default=30)

    args = parser.parse_args()
    if args.suite == "gpu":
        lock = None if args.dry_run else records.hold_lock()
        gpu.run(args.only, args.reps, args.seconds, args.cooldown, args.seed, args.node, args.dry_run)
    else:
        lock = records.hold_lock()
        web.run(args.prefix, args.users, args.warmup, args.seconds, args.cooldown, args.replicas, args.steps, args.step_seconds)
    if lock:
        lock.close()


if __name__ == "__main__":
    main()
