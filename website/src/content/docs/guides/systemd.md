---
title: systemd
description: Build the collector natively and run it as a service.
sidebar:
  order: 3
---

## Toolchain, once per machine

```sh
rustup toolchain install nightly --component rust-src
rustc +nightly --version --verbose | grep LLVM   # note the major version, 23 here

wget https://apt.llvm.org/llvm.sh && chmod +x llvm.sh && sudo ./llvm.sh 23
LLVM_SYS_231_PREFIX=/usr/lib/llvm-23 PATH=/usr/lib/llvm-23/bin:$PATH \
  cargo install bpf-linker --locked
```

`just` runs the recipes: `cargo install just` or the distribution package.

## Build and install

```sh
cd croft
just build-release
sudo install -m 0755 collector/target/release/croft /usr/local/bin/croft
sudo cp deploy/systemd/croft-collector.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now croft-collector

journalctl -u croft-collector -f
```

The dataset lands in `/var/lib/croft/croft-dataset.jsonl`. Set [variables](/croft/reference/configuration/) with `Environment=` lines in the unit, for example `Environment=WATCH_CGROUPS=docker`.

## Try it without installing

`just run` builds a debug binary and runs it under sudo in the foreground, writing to the same file, so `croft top` and `croft stats` read it without `--dataset`.

## Upgrade

```sh
git pull
just build-release
sudo install -m 0755 collector/target/release/croft /usr/local/bin/croft
sudo systemctl restart croft-collector
```

## Remove

```sh
sudo systemctl disable --now croft-collector
sudo rm /etc/systemd/system/croft-collector.service /usr/local/bin/croft
sudo systemctl daemon-reload
```

`/var/lib/croft` keeps the dataset.
