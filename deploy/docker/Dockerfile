# syntax=docker/dockerfile:1.7

FROM rust:1-slim-bookworm AS builder

WORKDIR /src

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

COPY --from=builder /src/collector/target/release/croft /usr/local/bin/croft

ENV RUST_LOG=info
ENV DATASET_PATH=/data/croft-dataset.jsonl

ENTRYPOINT ["/usr/local/bin/croft"]

CMD ["run"]
