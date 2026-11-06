# Task recipes for croft.

# Arguments reach recipes as $1, $2 and "$@", so quoted ones keep their spaces.
set positional-arguments

manifest := "collector/Cargo.toml"

# List available recipes.
list:
    @just --list

# Build the collector and its eBPF program.
build:
    cargo build --manifest-path {{ manifest }} -p collector

# Build in release mode.
build-release:
    cargo build --manifest-path {{ manifest }} -p collector --release

# Run the collector. Needs root to load and attach eBPF programs.
run:
    cargo build --manifest-path {{ manifest }} -p collector
    sudo RUST_LOG=info ./collector/target/debug/croft run

# Format check and clippy, userspace and shared crates only. Nightly: rustfmt.toml
# opts into unstable options that stable rustfmt silently ignores.
lint:
    cargo +nightly fmt --manifest-path {{ manifest }} --all -- --check
    cargo clippy --manifest-path {{ manifest }} -- -D warnings

# Apply formatting.
fmt:
    cargo +nightly fmt --manifest-path {{ manifest }} --all

# Run all tests.
test:
    cargo test --manifest-path {{ manifest }}

clean:
    rm -rf collector/target

# Run the collector image. WATCH_CGROUPS=kubepods also records every pod, GPU or not.
docker-up:
    docker run -d --name croft-collector --restart unless-stopped \
    --privileged --pid=host --cgroupns=host \
    -e WATCH_CGROUPS={{ quote(env_var_or_default("WATCH_CGROUPS", "")) }} \
    -v /sys/kernel/tracing:/sys/kernel/tracing:ro \
    -v /var/lib/croft:/data \
    croft-collector
# Sample applications of the experiments: just samples <recipe> [args]; just samples list for the recipes.
samples *args:
    @just --justfile labs/research/experiment/justfile --working-directory labs/research/experiment "$@"
