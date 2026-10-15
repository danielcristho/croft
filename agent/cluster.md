# Lab cluster

Three-node k3s cluster set up by `labs/k3s-cluster` (Ansible). The real host
names and addresses are in the gitignored `hosts.ini`; this file uses roles.

| Role | Notes |
|---|---|
| control plane | the machine the harness runs on; its dataset is local |
| worker | CPU node |
| GPU worker | NVIDIA device plugin |

## Access

```sh
export KUBECONFIG=~/.kube/croft-k3s.yaml   # the default kubeconfig does not reach it
```

| Component | Where |
|---|---|
| Ingress | ingress-nginx on NodePort **30080**; hosts as `<app>.127.0.0.1.sslip.io:30080` |
| Monitoring | kube-prometheus-stack in `monitoring`, Prometheus behind the same ingress |
| Mesh | Linkerd edge with Viz; Gateway API CRDs installed |
| croft | DaemonSet `croft-collector` in namespace `croft` |
| Kubelet | `image-gc-high-threshold=95`, eviction at `nodefs.available<5%` |

| Namespace | What runs there |
|---|---|
| `croft-exp` | the harness's `webapp` and its HPA |
| `boutique` | Online Boutique without its load generator, an HPA on every stateless service, ingress `boutique.127.0.0.1.sslip.io` |
| `cloud-dynamics` | cloud2023's `cpu-bound` and `stateless-api`, Linkerd-injected |

Node access goes through Ansible. Never accept an SSH host key automatically.

## Disk

The control plane's disk is tight. After every `docker build`, run
`docker builder prune -f`; build cache once filled it and the kubelet evicted
pods for DiskPressure. Check `df -h /` before pulling datasets or images.

Old evicted pods may linger as `ContainerStatusUnknown` in `croft-exp`. They are
harmless; delete them only with the user's approval.

## Images on the nodes

Images in no registry are loaded on every node:

```sh
docker build -t croft-webapp:v1 labs/research/experiment/workloads/webapp
docker builder prune -f
cd labs/k3s-cluster && ansible-playbook load-image.yml -e image=croft-webapp:v1 --become-password-file <file>
```

The user gives the become password. Write it to a mode-600 file in a scratch
directory, pass the file, and delete it right after the playbook. It never goes
into a repository file or onto a command line.

The harness refuses to run when a node's status does not list the image. After a
load, the node status can lag for minutes. To check the runtime itself, run a
pod with `--image-pull-policy=Never` pinned to the node; if it completes, the
image is there.

## Changes during experiments

Do not install addons, redeploy apps or change cluster settings while an
experiment runs: it pollutes the measurement. Idle apps in other namespaces are
fine.

## cloud2023 scripts

The course framework (`rm77/cloud2023`, cloned next to croft) has setup scripts
for OTel Collector, NetObserv and Inspektor Gadget under
`kubernetes/setup-cluster/supporting`, written for kind. Its `variables.sh` pins
the lecturer's `HOST_IP`. Run the scripts from a scratch directory with a local
`variables.sh` (`HOST_IP=127.0.0.1`) and point `PROMETHEUS_URL` at port 30080,
so its files stay untouched. Pull its updates with `git pull --ff-only`; the
local clone keeps a few fixes uncommitted (collector regex escape, a throttling
ratio metric, Linkerd queries on `pod` and `target_port`).
