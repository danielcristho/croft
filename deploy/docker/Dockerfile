# syntax=docker/dockerfile:1.7
#
# Build from the repo root: docker build -f deploy/docker/Dockerfile -t croft-collector .
#
# Run: needs a privileged container (eBPF touches the host kernel), the host PID
# namespace (backfill scans /proc for every host process, not just this container's),
# and a volume for the dataset file to survive container removal.
#
#   docker run --rm --privileged --pid=host \
#     -v /var/log/croft:/data \
#     croft-collector

FROM rust:1-slim-bookworm AS builder

WORKDIR /src

# Pin to the LLVM version nightly rustc embeds today (checked with
# `rustc +nightly --version --verbose`). bpf-linker fails at link time, not
# install time, if this drifts out of sync with a future nightly.
ARG LLVM_VERSION=23

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl wget ca-certificates gnupg lsb-release \
    software-properties-common \
    && rm -rf /var/lib/apt/lists/*

RUN rustup toolchain install nightly --component rust-src && rustup default nightly

RUN curl -fsSL https://apt.llvm.org/llvm.sh -o /tmp/llvm.sh \
    && chmod +x /tmp/llvm.sh \
    && /tmp/llvm.sh ${LLVM_VERSION}

ENV PATH="/usr/lib/llvm-${LLVM_VERSION}/bin:${PATH}"
ENV LLVM_SYS_231_PREFIX="/usr/lib/llvm-${LLVM_VERSION}"

RUN cargo install bpf-linker --locked

COPY . .
RUN cargo build --manifest-path collector/Cargo.toml -p collector --release

FROM debian:bookworm-slim

COPY --from=builder /src/collector/target/release/collector /usr/local/bin/collector

ENV RUST_LOG=info
ENV DATASET_PATH=/data/croft-dataset.jsonl

ENTRYPOINT ["/usr/local/bin/collector"]
