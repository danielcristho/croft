# Code style

Clean Code, SOLID and DRY apply to every part of croft: the Rust collector, the
Python harness and the notebooks.

## Tooling

- Rust: `just fmt` and `just lint`. Formatting needs nightly, because `rustfmt.toml`
  opts into unstable options that stable rustfmt ignores. Clippy runs with
  `-D warnings`.
- The pre-commit hook runs rustfmt, clippy and the tests when a staged change
  touches `collector/`. Without `bpf-linker` it skips the eBPF build.
- Python: `uv` for every project.
- Comments follow [code-conventions.md](code-conventions.md).

## Clean Code

- Write the smallest code that solves the actual problem. Delete before adding.
- Prefer boring and obvious over clever.
- Names carry the meaning, so most code needs no comment.
- A function does one thing at one level of abstraction.
- Files stay under 500 lines; split by responsibility.
- No dead code, no commented-out code, no `#[allow(dead_code)]`.
- No `unwrap()` or `expect()` in the collector's event path.
- No hardcoded thresholds, node names, namespaces or cloud resources in the
  pipeline; they come from configuration.

## SOLID, as croft reads it

Rust has no inheritance, so these are the Rust-native readings.

- **Single responsibility.** One crate or module, one pipeline stage:
  `collector -> feature -> analyzer -> decision -> platform/*`. In the harness:
  `kube.py` talks to kubectl, `prometheus.py` queries, `records.py` writes runs,
  `rps.py` sends load, `profiles.py` defines load shapes, `web.py` orchestrates.
- **Open/closed.** A trait or plug-in point only at a seam with a second real
  implementation: collector, extractor, analyzer, decision engine, platform
  client (Kubernetes, Docker, CloudX, Nomad). Elsewhere, the concrete type.
- **Liskov substitution.** An implementation owes its trait's contract, not just
  its signature. A test collector that never applies backpressure is not a
  stand-in for the eBPF one.
- **Interface segregation.** Small traits, one or two methods.
- **Dependency inversion.** No crate depends on one to its right; the decision
  crate depends on no platform crate, so it is testable without a cluster.

SOLID misapplied is a trait for every struct and a module for every function,
the opposite of clean code. A new trait names the second implementation that
needs it.

## DRY

One source of truth per piece of knowledge:

- constants and thresholds defined once (`POLL_MS`, `SLO_P95_MS`, `HPA_TARGET`);
- shared readers reused: `research/croftdata.py` for datasets, the harness's
  `kube`, `prometheus` and `records` modules;
- variants kept in one table: load shapes in `PROFILES`, probe settings in
  `PROBES`.

Extend an existing helper before writing a second one. Do not merge code that
only looks alike: DRY is about one piece of knowledge, not similar lines.

## Data in files, not in code

Lists and tunable values that change per application, per cluster or per
experiment live in a JSON file next to the code that reads it, not in the code:
framework libraries (`collector/collector/frameworks.json`), applications under
test, load profiles, probe settings, calibration numbers.

- The file is read once at startup and validated; a bad file stops the program
  with a clear error. Matching against it costs the same as a constant.
- In Rust the file is embedded as the default with `include_str!`, and an
  option with an env fallback replaces it (`--frameworks`, `CROFT_FRAMEWORKS`),
  so a ConfigMap changes it without a rebuild.
- Facts of a mechanism stay in code: kernel syscall numbers, `/proc` paths,
  units, metric names that form a format contract.

## Research code

- Anything that produced a number in a report is reproducible: pin the input,
  record the command. `runs.jsonl` does this for experiment runs.
- Label a heuristic as a heuristic. A threshold chosen after seeing the data is
  said to be so.
- Experiment code never becomes a dependency of the pipeline.

## Python and notebooks

- Python 3.12+, `uv` for every project (`uv sync`, `uv run`).
- Type hints on functions; dataclasses for records.
- Notebooks hold analysis, not logic shared with the harness; anything two
  notebooks need moves into a module.
- Plots: one finding per figure, labelled axes, the same palette across
  notebooks.
- Show diagrams in light mode, with minimal text and no crossing arrows.
- Never draw metric charts in ASCII. Harness docs follow the numbered-section
  style of `rm77/cloud2023`.
