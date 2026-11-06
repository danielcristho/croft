# Testing

Rust tests sit next to the code in `#[cfg(test)]` modules. Harness tests sit in
`labs/research/experiment/tests/`.

A test double owes the real behaviour, not just the signature. A test collector
that never applies backpressure is not a stand-in for the eBPF one.

Tests must not depend on a cluster or on today's date. Keep decisions in pure
functions, as in `decision/scaler.rs` and `compare.py`.

Before finishing a change:

1. Collector: run `just test` and `just lint`, and `just build` when the eBPF
   program or wiring changes.
2. Harness: run the tests by module from `labs/research/experiment`, because
   `unittest discover` does not find them:

   ```sh
   uv run python -m unittest tests.test_compare tests.test_kube tests.test_calibrate_scaler
   ```

3. Website: run `npm run build` in `website/` when its content changes.
4. Notebooks: run them through to the end, then read the output.
5. Probes: compare the counts with the collector before trusting them.
6. Reports: check every number in a table against its source.
