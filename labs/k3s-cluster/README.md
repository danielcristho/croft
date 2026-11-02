# croft Lab — k3s Cluster

This folder provides an Ansible playbook that provisions a **multi-node k3s cluster**, GPU nodes included, for the croft collector and the workloads it observes.

Every node has its own kernel, so the collector runs as one instance per node.

## Directory structure

```text
k3s-cluster/
├── README.md
├── ansible.cfg
├── hosts.example.ini      the inventory template
├── hosts.ini              your inventory, gitignored
├── group_vars/
│   └── all.yml            k3s version, CIDRs, kubelet arguments, ports
├── roles/
│   ├── common/
│   ├── server/
│   ├── agent/
│   └── gpu/
├── site.yml               provision or update the cluster
├── uninstall.yml          remove k3s from every node
├── load-image.yml         load a local Docker image into every node
└── collect.yml            pull croft's datasets from every node
```

---

# 1. Prerequisites

On the machine running Ansible:

```text
ansible-core
```

Install it:

```sh
uv tool install ansible-core
```

On every agent:

```text
SSH key access for a user with sudo
```

On GPU nodes:

```text
the NVIDIA driver
nvidia-container-toolkit
```

k3s finds the NVIDIA runtime on its own and creates the `nvidia` RuntimeClass.

---

# 2. Describe the nodes

Copy the inventory template:

```sh
cp hosts.example.ini hosts.ini
```

Fill in the hosts, their IPs and the user:

```ini
[server]
server-1 ansible_host=192.0.2.10 ansible_connection=local k3s_tls_sans='["100.64.0.10"]'

[agents]
agent-cpu-1 ansible_host=192.0.2.11
agent-gpu-1 ansible_host=192.0.2.12 ansible_become_exe=sudo.ws

[gpu]
server-1
agent-gpu-1
```

Cluster node names come from each host's hostname.

`hosts.ini` is gitignored. Do not commit it.

---

# 3. Provision the cluster

Run:

```sh
ansible-playbook site.yml --ask-become-pass
```

Point `kubectl` at the new cluster:

```sh
export KUBECONFIG=~/.kube/croft-k3s.yaml
```

The playbook is idempotent. A second run on a healthy cluster reports:

```text
changed=0
```

Run it again after editing `group_vars/all.yml` or `hosts.ini`.

---

# 4. Verify the cluster

Check the nodes:

```sh
kubectl get nodes -o wide
```

Every node must be `Ready`.

Check metrics-server, which the HPA needs:

```sh
kubectl top nodes
```

Check the GPUs:

```sh
kubectl get nodes -o 'custom-columns=NODE:.metadata.name,GPU:.status.allocatable.nvidia\.com/gpu'
```

Every GPU node must list at least one GPU.

---

# 5. Reach the cluster from outside the LAN

List the server's other addresses, such as its Tailscale IP, in `k3s_tls_sans` on the server line of `hosts.ini`:

```ini
server-1 ansible_host=192.0.2.10 ansible_connection=local k3s_tls_sans='["100.64.0.10"]'
```

Run the playbook again:

```sh
ansible-playbook site.yml --ask-become-pass
```

Point a copy of the kubeconfig at that address:

```sh
sed 's/192.0.2.10/100.64.0.10/' ~/.kube/croft-k3s.yaml > ~/.kube/croft-k3s-tailscale.yaml
```

Both kubeconfigs are cluster-admin. Do not commit or share them.

---

# 6. Add a node

Add it under `[agents]`, and under `[gpu]` if it has one.

Run the playbook again:

```sh
ansible-playbook site.yml --ask-become-pass
```

---

# 7. Load an image that is in no registry

Load a local Docker image into every node's containerd:

```sh
ansible-playbook load-image.yml -e image=deathstarbench/hotel-reservation:0.3.5 --ask-become-pass
```

From the experiment folder, the same as:

```sh
just samples load-image deathstarbench/hotel-reservation:0.3.5
```

---

# 8. Collect croft's datasets

Pull every node's croft segments of the last 24 hours, and its live file:

```sh
ansible-playbook collect.yml
```

Pull every segment a node still keeps:

```sh
ansible-playbook collect.yml -e collect_hours=0
```

The datasets land in:

```text
research/data/nodes/<node>/
```

The server's directory is linked, not copied.

---

# 9. What the playbook does

| Role | Hosts | Steps |
|---|---|---|
| `common` | all | raise inotify limits, download the installer, decide whether k3s needs (re)installing |
| `server` | `[server]` | open the k3s ports in ufw for the agents, write `config.yaml`, install k3s, fetch the kubeconfig |
| `agent` | `[agents]` | write `config.yaml` with the join token, install the agent, wait until the node is Ready |
| `gpu` | `[server]`, acting on `[gpu]` | label GPU nodes, apply the NVIDIA device plugin, wait for `nvidia.com/gpu` |

Each node is configured through:

```text
/etc/rancher/k3s/config.yaml
```

never through installer flags.

The installer runs only when:

```text
k3s is missing
k3s runs another version than k3s_version
its systemd unit still carries flags
```

---

# 10. Why the lab-specific steps exist

**inotify limits.**

The default of 128 instances runs out on hosts that also run Docker, Nomad or kind. The k3s CRI plugin then fails with `too many open files` and the node never registers.

**traefik and servicelb disabled.**

Both bind :80 and :443 on every node. Hosts that already serve those ports could not join. Install an ingress controller as a NodePort instead.

**ufw rules scoped to the agents.**

ufw drops forwarded traffic by default. The server allows 6443/tcp, 10250/tcp and 8472/udp from the agent addresses only, plus the pod and service CIDRs.

**No flags in the systemd unit.**

Flags and `config.yaml` are merged, so a value set in both, like `node-ip`, becomes a list of two and the kubelet refuses to start.

**Image GC at 95%, not 85%.**

Lab nodes are shared workstations whose disks sit above 85%. The kubelet then deletes every unused image each five minutes, including images loaded with `load-image.yml`, which no registry can restore.

**`ansible_become_exe=sudo.ws` on Ubuntu 26.04.**

Its default sudo is sudo-rs, whose password prompt Ansible cannot read.

**Device plugin with `runtimeClassName: nvidia`.**

Without it the plugin cannot load NVML and the node advertises no GPU.

---

# 11. Remove the cluster

Run:

```sh
ansible-playbook uninstall.yml --ask-become-pass
```

The inotify sysctl file stays. It is harmless without k3s.
