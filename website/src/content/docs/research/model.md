---
title: Scaler model
description: croft scale and the HPA written as equations.
sidebar:
  order: 2
---

`croft scale` as equations, taken from `collector/collector/src/decision/scaler.rs` and `command/scale.rs`, next to the HPA it is compared with.

## 1. Notation

| Symbol | Meaning | Source |
|---|---|---|
| `t` | poll index; polls are `Δ` apart | `--interval`, 2 s |
| `S(t)` | rate of the calibrated syscall, summed over the Deployment's containers | collectors' `/metrics` |
| `k` | syscalls per request | `--calls-per-request` |
| `c` | CPU seconds per request | `--cpu-per-request-ms` |
| `q` | CPU request of one replica, in cores | the Deployment's first container |
| `ρ` | target utilization | `--target`, 0.7 |
| `N_max` | replica ceiling | `--max-replicas`, 10 |
| `p` | polls that must agree before scaling up | `--confirm-polls`, 2 |
| `W` | scale-down window in polls, `⌈window / Δ⌉` | `--scale-down-window`, 300 s → 150 polls |
| `R(t)` | replicas running | the Deployment's `spec.replicas` |

## 2. Need

Request rate from the syscall rate:

```text
λ̂(t) = S(t) / k
```

Replicas needed to keep each replica at `ρ` of its CPU request:

```text
N(t) = clamp( ⌈ λ̂(t) · c / (q · ρ) ⌉ , 1 , N_max )
```

`k` and `c` only appear as the ratio `θ = c / k`, the CPU seconds per syscall:

```text
N(t) = clamp( ⌈ S(t) · θ / (q · ρ) ⌉ , 1 , N_max )
```

A calibration error in `N` comes from the error in `θ`, not from `k` or `c` alone.

## 3. Scale up

`Confirm` scales up once `p` polls in a row ask for more replicas than run:

```text
N(t − j) > R(t − j)   for j = 0, …, p − 1   ⇒   R(t + 1) = N(t)
```

The streak then starts again. One noisy poll scales nothing; the reaction delay is at most `p · Δ` = 4 s plus the time the Deployment takes to start a pod.

## 4. Scale down

`ScaleDown` keeps the last `W` needs and lowers replicas to the highest of them, once the window is full:

```text
M(t) = max_{j < W} N(t − j)
M(t) < R(t)   ⇒   R(t + 1) = M(t)
```

This is the HPA's stabilization window. A poll that misses a collector empties the window, since it undercounts `S`. Scale up wins when both fire in one poll.

## 5. The HPA

Every 15 s, from the mean CPU use `u` relative to the request:

```text
R(t + 1) = ⌈ R(t) · u(t) / ρ ⌉      skipped when |u / ρ − 1| ≤ 0.1
```

## 6. What the model shows

1. **Same sizing rule.** With `u = λ · c / (R · q)`, the HPA's rule becomes `⌈ λ · c / (q · ρ) ⌉`, which is croft's `N`. The two differ in where `λ` comes from and how often it is read, not in how replicas are sized.
2. **Both see a capped load.** A pod cannot use more CPU than its limit `ℓ` (without a limit, its node's free CPU), so `u ≤ ℓ / q`. `sendto` counts answered requests, so `S` is capped the same way, at about `R · ℓ · k / c`. One decision of either scaler can then grow replicas by at most a factor of `ℓ / (q · ρ)`.
3. **So croft's lead comes from timing.** It reads the rate every 2 s, from the kernel, while the HPA waits for the metrics API every 15 s. That matches the results: croft reacts in 0–2 s, the HPA in 19–34 s. It does not predict a burst, and a large burst still takes several steps.
4. **The missing signal is the offered load.** Lifting the cap in point 2 needs a signal that counts requests not yet answered, such as the accept queue (issue #8).
