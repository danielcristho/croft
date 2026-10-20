# Dataset, signals and results

## Row format

One JSON line per process per 2 s poll:

| Field | Holds |
|---|---|
| `unix_ms`, `tgid` | time and process |
| `process` | `comm`, `exe`, `cgroup`, `container_id`, `cwd` (working directory, names the project of a host process), `framework` (see `frameworks.json`; left out when none) |
| `workload` | `gpu-onset`, `gpu-held` or `no-gpu` |
| `usage` | process CPU, RSS, threads, context switches, I/O and network bytes per second, `runqueue_s` |
| `cgroup_usage` | `cpu_cores`, `cpu_throttled_fraction`, `memory_bytes`, `oom_kills`, PSI `*_pressure_avg10` |
| `syscalls` | list of `{name, rate_per_sec}` |
| `label` | set by labelled experiments |

Idle processes are written every 15 polls. Empty GPU fields are left out.
A process that held a GPU fd before the collector started reads as `gpu-held` with `ioctl_count` 0.

Pre-filter large files with `grep -F -f ids.txt` on container ids before parsing
JSON; parsing tens of gigabytes in Python takes many minutes.

## Signals for web services

| Signal | Use |
|---|---|
| `sendto` rate ÷ 2.06 | request rate as the kernel sees it; correlation about 0.8 with the load generator |
| `recvfrom`, `epoll_wait` | also about one per request |
| `accept4` | not usable: ingress-nginx keeps upstream connections alive, new connections are rare |
| `network_rx_bytes_s` | tracks requests; counted per network namespace, so take the max per pod |
| `cpu_pressure_avg10 > 10` or `cpu_throttled_fraction > 0.2`, two polls | pressure rule of the lead-time notebook |
| `runqueue_s` | seconds per second the threads waited for a CPU, summed over threads (rq time, Landau et al. 2026); a 2 s window where PSI averages 10 s. Checked on one host: 4 CPU-bound processes on 1 CPU read 3.00, by quota or by pinning; one alone reads 0. On the burst4-croft bursts it fires with `sendto` and PSI (0.1–2.3 s), tracks throttling per pod (r 0.80, 1.8× higher) and climbs with the pod count, so it does not size a burst |
| request rate × CPU per request ÷ (limit × 0.7) | projected utilization, and the replicas a burst needs |

The webapp costs 45–58 ms of CPU per request; node calibration gives 38–64 ms
and speed factors 1.000 / 0.792 / 0.597 (`data/node-speed.json`).

`sendto` counts answered requests. Once a pod saturates, it shows the pod's
capacity, not the offered load, and the replica estimate comes out low. The
missing signal is the queue of connections waiting to be accepted.

## Notebooks

`research/` holds the notebooks, `croftdata.py` (reads per-node datasets) and a
JupyterLab container (`docker compose up -d`, port 8890, token in `.env`).

| Notebook | Question |
|---|---|
| `lead_time.ipynb` | how far ahead of the HPA and the SLO the pressure signal rises, on steps and ramps |
| `burst_signals.ipynb` | which syscall tracks requests; reaction time per signal; open-loop bursts |

Outside the container, set the paths:

```sh
cd research
CROFT_DATASET=$PWD/data/nodes EXPERIMENT_DATA=$PWD/../labs/research/experiment/data \
  uv run jupyter nbconvert --to notebook --execute --inplace burst_signals.ipynb
```

Parsed rows are cached in `out/`, keyed by a fingerprint of the dataset files. A
new collect invalidates the cache, and the next run rereads everything.

## Results so far

| Load | Result |
|---|---|
| Ramp (Locust) | pressure signal warns 80 ± 20 s before p95 > 500 ms |
| Step 2 → 4 users | request rate reacts in about 1 s, pressure in 1–31 s, HPA in 19–34 s |
| Step to 8 or 12 users | closed-loop load hides the burst; the SLO breaks within 0–5 s |
| Open-loop burst 5 → 75 RPS, default probes | croft request rate reacts in 0–2 s; the HPA never scaled: the overloaded pod failed its probes and restarted, and the metrics API returned nothing; about 96% of burst requests failed |
| CPU heterogeneity | the same request costs 1.69× more CPU on the slowest node; the HPA averaged 67% while one replica throttled 34% |

The ramp rule (the episode in progress when the SLO breaks) was chosen after
seeing the data; say so when citing it.
