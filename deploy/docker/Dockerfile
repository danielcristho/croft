# syntax=docker/dockerfile:1.7

FROM rust:1-slim-bookworm@sha256:7c7d4eb4d05b7781187a327809466cfddf29753f6defdc3d19a0e291951b7a37 AS builder

WORKDIR /src

ARG LLVM_VERSION=23

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl wget ca-certificates gnupg lsb-release \
    software-properties-common \
    && rm -rf /var/lib/apt/lists/*

RUN rustup toolchain install nightly --component rust-src && rustup default nightly

# llvm.sh no longer accepts Debian 12, but apt.llvm.org still builds LLVM for it; add the repository it would add.
# The packages are the ones llvm.sh installs, recommends included, so bpf-linker finds the same LLVM.
RUN curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key -o /etc/apt/trusted.gpg.d/apt.llvm.org.asc \
    && echo "deb http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-${LLVM_VERSION} main" > /etc/apt/sources.list.d/llvm.list \
    && apt-get update \
    && apt-get install -y clang-${LLVM_VERSION} lldb-${LLVM_VERSION} lld-${LLVM_VERSION} clangd-${LLVM_VERSION} \
    && rm -rf /var/lib/apt/lists/*

ENV PATH="/usr/lib/llvm-${LLVM_VERSION}/bin:${PATH}"
ENV LLVM_SYS_231_PREFIX="/usr/lib/llvm-${LLVM_VERSION}"

RUN cargo install bpf-linker --locked

COPY . .

RUN cargo build --manifest-path collector/Cargo.toml -p collector --release

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc

COPY --from=builder /src/collector/target/release/croft /usr/local/bin/croft

ENV RUST_LOG=info
ENV DATASET_PATH=/data/croft-dataset.jsonl

ENTRYPOINT ["/usr/local/bin/croft"]

CMD ["run"]
