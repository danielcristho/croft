# Architecture

## Pipeline Rules

Keep the dependency flow:

```
kernel tracepoints (eBPF) -> collector -> dataset (JSON Lines, 2 s poll)
                                       -> /metrics (per container)
                                              -> croft scale -> Deployment replicas
```

A stage never depends on one to its right. `decision/` depends on no platform
code, so it can be tested without a cluster.

**Known limit.** `croft scale` only adds replicas, and the HPA next to it can undo
them. The final series needs scale-down in `croft scale` (#44).

- One module, one pipeline stage.
- Add a trait only at a seam with a second real implementation.
- No hardcoded thresholds, node names or namespaces in the pipeline.
- Experiment code never becomes a dependency of the pipeline.

## Harness Modules

| Module | Responsibility |
|---|---|
| `kube.py` | talks to kubectl |
| `prometheus.py` | queries Prometheus |
| `records.py` | writes runs |
| `rps.py` | sends open-loop load |
| `profiles.py` | defines load shapes |
| `apps.py` | reads `apps.json` |
| `scaler.py` | deploys `croft scale` for a run |
| `calibrate_scaler.py` | measures an application's calibration |
| `compare.py` | scores finished runs |
| `web.py` | orchestrates a run |

## Adding Or Changing A Command

1. Add its arguments to `collector/collector/src/cli.rs`.
2. Add `command/<name>.rs` with an `execute` function, and declare it in
   `command/mod.rs`.
3. Dispatch it in `main.rs`.
4. Keep decisions in `decision/`, so they stay testable without a cluster.

## Adding An Application

1. Add `workloads/<app>/k8s.yaml` and `workloads/<app>/hpa.yaml`.
2. Add its entry to `apps.json`.
3. Measure its calibration with `just samples calibrate <app>`, and validate it.
4. Check it with `just samples deploy <app>` and `just samples smoke <app>`.

## Adding A Load Profile

Add the shape to `PROFILES` in `profiles.py`, and its rate to `Rate.at`. Rates are
multiplied by the application's `scale`.
