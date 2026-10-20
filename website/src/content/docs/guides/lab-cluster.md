---
title: Lab cluster
description: Provision the multi-node k3s cluster the experiments run on.
sidebar:
  order: 4
---

`labs/k3s-cluster` is an Ansible playbook that provisions a multi-node k3s cluster, GPU nodes included, for the collector and the workloads it observes.

## Requirements

- On the machine running Ansible: `ansible-core` (`uv tool install ansible-core`).
- On every agent: SSH key access for a user with sudo.
- On GPU nodes: the NVIDIA driver and `nvidia-container-toolkit`. k3s finds the runtime on its own and creates the `nvidia` RuntimeClass.

## Provision

```sh
cd labs/k3s-cluster
cp hosts.example.ini hosts.ini        # gitignored; fill in hosts, IPs and user
ansible-playbook site.yml --ask-become-pass
export KUBECONFIG=~/.kube/croft-k3s.yaml
kubectl get nodes
```

The playbook is idempotent: a second run on a healthy cluster reports `changed=0`. To add a node, list it under `[agents]` (and `[gpu]` if it has one) and run it again. `uninstall.yml` removes everything.

| Role | Hosts | Steps |
|---|---|---|
| `common` | all | raise inotify limits, download the installer, decide whether k3s needs (re)installing |
| `server` | `[server]` | open the k3s ports in ufw for the agents, write `config.yaml`, install k3s, fetch the kubeconfig |
| `agent` | `[agents]` | write `config.yaml` with the join token, install the agent, wait until the node is Ready |
| `gpu` | `[server]`, acting on `[gpu]` | label GPU nodes, apply the NVIDIA device plugin, wait for `nvidia.com/gpu` |

## Other playbooks

| Playbook | Does |
|---|---|
| `load-image.yml -e image=<name> -K` | loads a local Docker image into every node's containerd, for images in no registry |
| `collect.yml` | pulls each node's recent dataset segments into `research/data/nodes/<node>/` |

## Why the lab-specific steps exist

- **inotify limits.** The default of 128 instances runs out on hosts that also run Docker, Nomad or kind; the k3s CRI plugin then fails with `too many open files` and the node never registers.
- **traefik and servicelb disabled.** Both bind :80 and :443 on every node; hosts that already serve those ports could not join.
- **ufw rules scoped to the agents.** The server allows 6443/tcp, 10250/tcp and 8472/udp from the agent addresses only, plus the pod and service CIDRs.
- **No flags in the systemd unit.** Flags and `config.yaml` are merged, so a value set in both becomes a list of two and the kubelet refuses to start.
- **Image GC at 95%, not 85%.** Lab nodes are shared workstations whose disks sit above 85%; the kubelet would otherwise delete loaded images that no registry can restore.
- **`ansible_become_exe=sudo.ws` on Ubuntu 26.04.** Its default sudo is sudo-rs, whose password prompt Ansible cannot read.
- **Device plugin with `runtimeClassName: nvidia`.** Without it the plugin cannot load NVML and the node advertises no GPU.
