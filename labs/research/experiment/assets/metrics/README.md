# Metric figures · hotel-reservation

From the hotel-reservation series of 2026-11-02 (13:36–15:14 WIB): three autoscalers,
three runs each, one burst of 20 → 300 RPS for 120 s per run.

These figures record the earlier series, which still included KEDA. The final series compares the HPA with croft alone.

| File | Caption |
|---|---|
| `hr-series-requests.png` | The same burst is sent nine times: HPA, KEDA and croft, three rounds in that order. |
| `hr-series-replicas.png` | Pods per run across the series: HPA adds 1–2 pods, KEDA 3, croft 3–4. |
| `hr-none-1-replicas.png`, `hr-keda-1-replicas.png`, `hr-croft-1-replicas.png` | Replicas in one run per autoscaler. |
| `hr-croft-1-cpu-per-pod.png` | croft run: the first pod's CPU jumps at the burst, then drops once four new pods share the load. |
| `hr-croft-1-cpu-and-croft-rps.png` | croft run per pod: CPU (top) and croft's estimated requests per second against client successes (bottom). |
| `hr-reaction-timeline.png` | croft asks for a new pod about 6 s into the burst, KEDA about 22 s, the HPA 20–40 s. The HPA running next to croft lowers croft's replicas again (×). |
| `hr-reaction-per-run.png` | Reaction per run: first scale-up, first new pod Ready, and time to peak replicas. One dot per run; the bar is the mean. |

Grafana panels come from the `croft-experiment` dashboard; the rest from `research/experiment_metrics.ipynb`.
