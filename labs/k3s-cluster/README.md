# k3s cluster for croft experiments

Ansible playbook that provisions a multi-node k3s cluster, GPU nodes included,
for the croft collector and the workloads it observes. Every node has its own
kernel, so the collector runs as one instance per node.

## Requirements

- On the machine running Ansible: `ansible-core` (`uv tool install ansible-core`).
- On every agent: SSH key access for a user with sudo.
- On GPU nodes: the NVIDIA driver and `nvidia-container-toolkit`. k3s finds the
  runtime on its own and creates the `nvidia` RuntimeClass.

## Usage

```sh
cp hosts.example.ini hosts.ini        # gitignored; fill in hosts, IPs and user
ansible-playbook site.yml --ask-become-pass
export KUBECONFIG=~/.kube/croft-k3s.yaml
kubectl get nodes
```

The playbook is idempotent: a second run on a healthy cluster reports
`changed=0`. Run it again after editing `group_vars/all.yml` or `hosts.ini`.

Adding a node: add it under `[agents]` (and `[gpu]` if it has one), then rerun.

Removing everything: `ansible-playbook uninstall.yml --ask-become-pass`. The
inotify sysctl file stays; it is harmless without k3s.

## What it does

| Role | Hosts | Steps |
|---|---|---|
| `common` | all | raise inotify limits, download the installer, decide whether k3s needs (re)installing |
| `server` | `[server]` | open the k3s ports in ufw for the agents, write `config.yaml`, install k3s, fetch the kubeconfig |
| `agent` | `[agents]` | write `config.yaml` with the join token, install the agent, wait until the node is Ready |
| `gpu` | `[server]`, acting on `[gpu]` | label GPU nodes, apply the NVIDIA device plugin, wait for `nvidia.com/gpu` |

Each node is configured through `/etc/rancher/k3s/config.yaml`, never through
installer flags. The installer runs only when k3s is missing, on another
version than `k3s_version`, or when its systemd unit still carries flags.

## Why the lab-specific steps exist

- **inotify limits.** The default of 128 instances runs out on hosts that also
  run Docker, Nomad or kind; the k3s CRI plugin then fails with
  `too many open files` and the node never registers.
- **traefik and servicelb disabled.** Both bind :80 and :443 on every node;
  hosts that already serve those ports could not join. Install an ingress
  controller as a NodePort instead.
- **ufw rules scoped to the agents.** ufw drops forwarded traffic by default.
  The server allows 6443/tcp, 10250/tcp and 8472/udp from the agent addresses
  only, plus the pod and service CIDRs.
- **No flags in the systemd unit.** Flags and `config.yaml` are merged, so a
  value set in both, like `node-ip`, becomes a list of two and the kubelet
  refuses to start.
- **`ansible_become_exe=sudo.ws` on Ubuntu 26.04.** Its default sudo is
  sudo-rs, whose password prompt Ansible cannot read.
- **Device plugin with `runtimeClassName: nvidia`.** Without it the plugin
  cannot load NVML and the node advertises no GPU.

## Checks after provisioning

```sh
kubectl get nodes -o wide
kubectl top nodes                                             # metrics-server, needed by HPA
kubectl get nodes -o 'custom-columns=NODE:.metadata.name,GPU:.status.allocatable.nvidia\.com/gpu'
```
