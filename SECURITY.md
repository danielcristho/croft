# Security policy

## Reporting a vulnerability

Do not open a public issue. Report it privately at <https://github.com/danielcristho/croft/security/advisories/new>, or open the repository's **Security** tab and choose **Report a vulnerability**.

Include the affected version or commit, the kernel version, the steps to reproduce and the impact. Maintainers work on a best-effort basis, will reply as soon as they can, and will agree a disclosure date with you.

## Supported versions

croft has no stable release. Only the `main` branch receives fixes.

## What to keep in mind

The collector runs as a privileged DaemonSet and loads eBPF programs into the kernel, so a flaw in it can affect the whole node. `croft scale` holds permission to scale Deployments.

The eBPF programs do not read syscall buffers or environment variables. The socket address passed to `connect` is the only argument they read from user memory for operand metrics, so destination addresses and ports appear in the metrics. Treat the metrics endpoint as internal.
