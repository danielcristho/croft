# croft Agent Guide

This guide applies to the whole repository. Use it as the main context when
analysing, changing and verifying the code.

The content is split across `agent/`. Read the first four files before changing
code; read the others when the task touches their topic.

| File | Holds |
|---|---|
| [agent/project.md](agent/project.md) | Stack, repository layout, applications under test, environment variables, development commands |
| [agent/architecture.md](agent/architecture.md) | Pipeline rules, how to add a command, an application or a load profile |
| [agent/code-style.md](agent/code-style.md) | Clean Code, SOLID, DRY, tooling, data in files, research code |
| [agent/code-conventions.md](agent/code-conventions.md) | Comment conventions |
| [agent/commit.md](agent/commit.md) | Commit and pull request rules |
| [agent/security.md](agent/security.md) | Credentials, cluster access, eBPF probe limits |
| [agent/testing.md](agent/testing.md) | Tests and verification before finishing a change |
| [agent/change-discipline.md](agent/change-discipline.md) | Limits on changes |
| [agent/experiments.md](agent/experiments.md) | Experiment arms, running a series, load, datasets, output |
| [agent/signals.md](agent/signals.md) | Dataset format, signals, notebooks, results |
| [agent/cluster.md](agent/cluster.md) | Lab cluster, access, disk, node images |

The results so far support "croft reacts earlier". They do not support "croft
predicts a burst" or "croft improves the SLO", so do not write those claims.

Reply to the user in plain, short Indonesian with the idea first. Work as a
critical peer: question weak claims and give a recommendation.
