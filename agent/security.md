# Security

## Credentials

- Never commit or share `~/.kube/croft-k3s*.yaml`: they are cluster-admin.
- `hosts.ini`, `.env` and `research/` stay out of the repository.
- Real hosts, addresses, usernames and passwords stay out of committed files and
  command lines.
- Do not print tokens or passwords, or store them in files or logs. Read
  credentials into environment variables.
- Never ask for, use or store a node's sudo password. The user runs playbooks that
  need it, as in [cluster.md](cluster.md).
- Do not send the user's email address to external services.

## Cluster Access

- Node access goes through Ansible. Never accept an SSH host key automatically.
- Do not change `sshd` or `authorized_keys` without approval.
- Ask before deleting cluster resources or images the user made.

## eBPF Probes

- Never read syscall buffers or environment variables. Record sizes, fd types and
  destinations.
- `execve` stays out of operand recording: it is rare while serving, and its
  environment can hold secrets.
- A privileged probe on a cluster node needs the user's approval for each set of
  nodes. Give the pod an `activeDeadlineSeconds`, and delete it afterwards.
