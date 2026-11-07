---
title: Results
description: What the lab experiments have shown so far.
sidebar:
  order: 1
---

Measured on the three-node [lab cluster](/guides/lab-cluster/) with the [burst harness](/guides/experiments/), against the HPA at a 70% CPU target.

| Load | Result |
|---|---|
| Ramp (Locust) | the pressure signal warns 80 ± 20 s before p95 latency passes 500 ms |
| Step from 2 to 4 users | request rate reacts in about 1 s, pressure in 1–31 s, the HPA in 19–34 s |
| Step to 8 or 12 users | closed-loop load hides the burst; the SLO breaks within 0–5 s |
| Open-loop burst, 5 → 75 RPS, default probes | croft's request rate reacts in 0–2 s; the HPA never scaled: the overloaded pod failed its probes and restarted, and the metrics API returned nothing; about 96% of burst requests failed |
| CPU heterogeneity | the same request costs 1.69× more CPU on the slowest node; the HPA averaged 67% while one replica throttled 34% |

The ramp rule (the episode in progress when the SLO breaks) was chosen after seeing the data; say so when citing it.

## Calibration

The lab webapp costs 45–58 ms of CPU per request. Node calibration gives 38–64 ms and speed factors 1.000, 0.792 and 0.597.

## Open questions

- `sendto` counts answered requests, so a saturated pod shows its capacity, not the offered load. The missing signal is the queue of connections waiting to be accepted (issue #8).
- `runqueue_s` fires with `sendto` and PSI on bursts (0.1–2.3 s) and tracks throttling per pod, but climbs with the pod count, so it does not size a burst.
- The scaler runs next to the HPA (`--scaler croft`) but is not yet validated against it (issue #9).

## Notebooks

| Notebook | Question |
|---|---|
| `research/lead_time.ipynb` | how far ahead of the HPA and the SLO the pressure signal rises, on steps and ramps |
| `research/burst_signals.ipynb` | which syscall tracks requests; reaction time per signal; open-loop bursts |
