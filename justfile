# Task recipes for CloudX autoscaler.

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
    sudo -E ./collector/target/debug/collector

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
