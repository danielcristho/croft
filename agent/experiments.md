# Experiments

The harness is `labs/research/experiment` (`run.py`, `uv`). Its README, written
in cloud2023's style, lists every command; this file holds what the README does
not: how to run series safely and the traps met so far.

## A burst series

```sh
cd labs/research/experiment
export KUBECONFIG=~/.kube/croft-k3s.yaml
for i in 1 2 3; do
  uv run run.py web --prefix burst-$i --profile burst --scale 0.5 || exit 1
done
```

| Option | Meaning |
|---|---|
| `--profile constant\|ramp\|burst\|periodic\|variable` | open-loop requests per second, cloud2023's scenario parameters |
| `--scale` | multiplies every rate; burst at 0.5 is 5 → 75 RPS, about 6–7 pods at the HPA's 70% |
| `--steps 2,4,8,12 [--shape ramp]` | Locust users, closed loop |
| `--probes relaxed` | no liveness probe, readiness tolerates 60 s; an HPA baseline not killed by its own probes |
| `--replicas N` | fixed replicas instead of the HPA |

Every run redeploys at one replica and recreates the HPA, so runs carry no state
over. One experiment runs at a time (`data/experiment.lock`).

## Running a series

- Run it in the background and wait for its completion notice; a burst run takes
  about 11–12 minutes.
- Do not pipe the loop through `grep` or `tail`. The pipe hides the exit code,
  and a series that stopped at its first run looks like it succeeded. Read the
  output and check that `data/web/<run>/` exists for every run.
- Do not edit the harness while a series runs. Each repetition starts a new
  Python process and loads whatever the files hold at that moment.
- Test a Kubernetes patch with `--dry-run=server` before a series depends on it.

## Load: open loop for bursts

Locust users are closed loop: each waits for its answer before sending again.
Once the app saturates, more users mean longer queues, not more requests, and
the burst hides behind the app's own slowness. Use `--profile` (open loop, fixed
offered rate) for bursts; keep Locust for steps and ramps.

## Collecting datasets

```sh
cd labs/k3s-cluster
ansible-playbook collect.yml --limit <worker>,<gpu-worker>
```

Limit the playbook to the remote nodes. The control plane's file in
`research/data/nodes/` is a symlink to its live dataset; rsync would replace it
with a copy of many gigabytes. Remote datasets grow to tens of gigabytes; check
the disk first.

## Output

```text
labs/research/experiment/data/web/
├── runs.jsonl          one line per run: load, probes, window, containers
└── <run>/
    ├── workload.csv    open-loop runs: offered and achieved RPS, p95 per second
    ├── locust_*.csv    Locust runs
    ├── prometheus.csv  CPU, throttling, memory, network, restarts, replicas
    └── timeline.json   HPA rescale events, pod created and Ready times
```

A run's croft rows are found by the container ids in `runs.jsonl`, never by time
alone.
