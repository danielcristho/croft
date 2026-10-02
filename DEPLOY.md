# Deploying the collector to a node

From a bare Ubuntu/Debian node with no Rust, no LLVM, nothing. Run everything
below on the target node itself, not on your dev machine.

## 1. Copy the code

No GitHub Actions or release artifact yet, so copy the working tree directly.
From your dev machine:

```bash
rsync -av --exclude target croft/ <user>@<node>:~/croft/
```

## 2. Build tools

Every crate with a build script (`bpf-linker` included) needs a C
linker. A bare node usually does not have one:

```bash
sudo apt-get update && sudo apt-get install -y build-essential
```

## 3. Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly --component rust-src
```

`collector-ebpf` only builds under nightly, with `-Z build-std`. The stable
toolchain builds `collector` and `collector-common` fine on its own; `cargo`
picks nightly automatically for the eBPF crate through `collector/build.rs`.

## 4. Match LLVM to nightly's LLVM

`bpf-linker` links against LLVM's bitcode reader. If its LLVM major version
does not match the one nightly rustc embeds, linking fails at build time with
`ERROR llvm: Invalid record`, not at install time, so check this before
installing bpf-linker rather than after it fails.

```bash
rustc +nightly --version --verbose | grep LLVM
```

Install the matching version (bpf-linker 0.11 supports LLVM 21, 22, or 23; if
nightly reports something else, use the closest one in that range):

```bash
curl -fsSL https://apt.llvm.org/llvm.sh -o /tmp/llvm.sh
chmod +x /tmp/llvm.sh
sudo /tmp/llvm.sh 23   # replace 23 with the version from the grep above
```

## 5. Install bpf-linker

`llvm-config` needs to be reachable **unversioned** on `PATH`. apt only
installs the versioned name (`llvm-config-23`) under `/usr/bin`; the
unversioned one lives next to it under `/usr/lib/llvm-<version>/bin`.

```bash
export PATH=/usr/lib/llvm-23/bin:$PATH
export LLVM_SYS_231_PREFIX=/usr/lib/llvm-23
cargo install bpf-linker --locked
```

Add both `export` lines to `~/.bashrc` (or `~/.zshrc`), replacing `23`/`231`
with whatever version step 4 actually installed, so they hold on every future
login, not just this shell.

## 6. Install `just`

```bash
cargo install just
```

## 7. Build and run

```bash
cd ~/croft
just build
sudo RUST_LOG=info ./collector/target/debug/collector
```

Loading and attaching eBPF programs needs root (or `CAP_BPF`+`CAP_PERFMON`).
Setting `RUST_LOG` as part of the `sudo` command, not before it, matters: some
sudoers configs reject `-E` outright ("preserving the entire environment is
not supported"), silently dropping `RUST_LOG` and leaving env_logger silent
with no error. `sudo RUST_LOG=info <cmd>` sets the variable for sudo itself to
pass through, and works regardless of that setting.

## 8. Verify the signal

In another shell on the same node, drive an actual GPU:

```bash
python3 -c "import torch; torch.cuda.init()"
```

or start a `single-gpu`/`multi-gpu` cloudx workspace on this node. The
collector's terminal should print a `workload classified: gpu-onset tgid=<pid>
ioctl_count=<n> top_syscalls=[...]` line within a couple of seconds, and a
matching row in `croft-dataset.jsonl`.

Then verify the negative case: run a plain CPU workload (anything that never
touches `/dev/nvidia*`/`/dev/dri/*`) and confirm no such line appears for its
pid.

Ctrl-C stops the collector; it detaches its programs on exit.

## 9. Run continuously (systemd)

For unattended data collection, install it as a service rather than keeping a
terminal open. Edit `WorkingDirectory` and `ExecStart` in
`deploy/systemd/croft-collector.service` first if the repo isn't at
`/var/www/croft`.

```bash
sudo cp deploy/systemd/croft-collector.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now croft-collector
```

```bash
journalctl -u croft-collector -f   # follow logs
sudo systemctl restart croft-collector
sudo systemctl stop croft-collector
```

`Restart=always` in the unit means a crash restarts it automatically; no
Ctrl-C-driven babysitting needed.
