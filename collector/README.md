# collector

## Prerequisites

1. stable rust toolchains: `rustup toolchain install stable`
1. nightly rust toolchains: `rustup toolchain install nightly --component rust-src`
1. (if cross-compiling) rustup target: `rustup target add ${ARCH}-unknown-linux-musl`
1. (if cross-compiling) LLVM: (e.g.) `brew install llvm` (on macOS)
1. bpf-linker: `cargo install bpf-linker` (`--no-default-features` on macOS)

## Build & Run

Use `cargo build`, `cargo check`, etc. as normal. Run your program with:

```shell
cargo run --release
```

Cargo build scripts are used to automatically build the eBPF correctly and include it in the
program.

## Cross-compiling on macOS

Cross compilation should work on both Intel and Apple Silicon Macs.

```shell
cargo build --package collector --release \
  --target=${ARCH}-unknown-linux-musl \
  --config=target.${ARCH}-unknown-linux-musl.linker=\"rust-lld\"
```
The cross-compiled program `target/${ARCH}-unknown-linux-musl/release/collector` can be
copied to a Linux server or VM and run there.

## License

The collector, its CLI and `collector-common` are licensed under the Apache License 2.0, as is the rest of
croft; see [LICENSE](../LICENSE).

The eBPF programs in `collector-ebpf` are licensed under either the [GNU General Public License, Version 2]
or the [MIT license], at your option, because the kernel helpers they call need a GPL-compatible license.
Contributions to them are dual licensed the same way.

The workspace started from [aya-template](https://github.com/aya-rs/aya-template), Copyright (c) 2021
Alessandro Decina, under the MIT license; `LICENSE-MIT` keeps that notice.

[MIT license]: LICENSE-MIT
[GNU General Public License, Version 2]: LICENSE-GPL2
