# Experiments

The harness is `labs/research/experiment` (`run.py`, `uv`). Its README, written
in cloud2023's style, lists every command; this file holds what the README does
not: how to run series safely and the traps met so far.

## Arms

| `--scaler` | Who adds replicas | Signal |
|---|---|---|
| `none` | the HPA | CPU at 70% of the request, through metrics-server |
| `croft` | `croft scale`, with no HPA | calibrated syscall rate from the collector's `/metrics` |
| `croft-step` | `croft scale`, with no HPA, capped at +4 pods or +100% per 15 s | as `croft` |

Both arms scale down after a 300 s window. Runs before this change had the HPA
next to `croft scale`, so do not pool them with new runs.

## A burst series

```sh
just samples series hotel-reservation hr
```

| Option | Meaning |
|---|---|
| `--profile constant\|ramp\|burst\|periodic\|variable` | open-loop requests per second, cloud2023's scenario parameters |
| `--scale` | multiplies every rate; default the app's `scale` in `apps.json` |
| `--steps 2,4,8,12 [--shape ramp]` | Locust users, closed loop |
| `--probes relaxed` | no liveness probe, readiness tolerates 60 s; an HPA baseline not killed by its own probes |
| `--probes none` | no probes; the HPA's best case |
| `--app NAME` | an application of `apps.json`; `hotel-reservation` by default |
| `--scaler croft` | `croft scale` instead of the HPA for the run; needs `--profile` and the collector's `/metrics` |
| `--replicas N` | fixed replicas instead of the HPA |

Every run redeploys at one replica and recreates the HPA, so runs carry no state
over. One experiment runs at a time (`data/experiment.lock`).

## Running a series

- Run it in the background and wait for its completion notice; a burst run takes
  about 17 minutes.
- Do not pipe the loop through `grep` or `tail`. The pipe hides the exit code,
  and a series that stopped at its first run looks like it succeeded. Read the
  output and check that `data/web/<run>/` exists for every run.
- `kube-state-metrics` is scraped every 30 s and misses fast replica changes. Take
  replica metrics from scaling decisions.
- Do not edit the harness while a series runs. Each repetition starts a new
  Python process and loads whatever the files hold at that moment.
- Test a Kubernetes patch with `--dry-run=server` before a series depends on it.
- A finished pod (`Succeeded`) under the app's selector freezes the HPA: it counts
  as a replica with 0% CPU, the average falls below target and no scale-up
  happens. The six `burst-*` runs before 2026-10-17 had one; their "HPA never
  scaled" result is not usable. The harness now refuses to run while finished
  pods exist.
- Every deploy restarts the pod. Without probes, an overloaded pod is never
  restarted, and the next run would inherit hundreds of queued requests.

## croft scale against the HPA

Pair the arms: the HPA alone and `--scaler croft`, same profile, scale and `--probes relaxed`,
three repetitions each, then score them with `just samples compare`. Per-run scores go to
`data/web/compare.csv`. Check for croft climbing in small steps, since a syscall shows a
saturated pod's capacity, not the offered load (issue #11).

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
