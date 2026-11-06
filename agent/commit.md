# Commits

`.githooks/commit-msg` enforces Conventional Commits on the first line:

```text
type(scope): description
```

## Rules

- **Type**: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`,
  `perf`, `ci`, `build`, `revert`.
- **Scope**, optional:

  | Scope | Covers |
  |---|---|
  | `collector` | the Rust collector, eBPF program and `croft` CLI |
  | `labs` | cluster setup and the experiment harness |
  | `apps` | applications under test |
  | `research` | analysis notebooks and their helpers |

  Omit it for repository-wide changes.
- **Description**: lowercase, imperative or noun phrase, no trailing period, at
  most 72 characters. It says what changed and, when it fits, why.
- One line. No body, no co-author or attribution line.
- One logical change per commit.

## Examples from the history

```text
feat(collector): idle heartbeat and skip empty gpu fields to cut dataset growth
feat(collector): croft profile for per-container resource profiles
fix(collector): exit on SIGINT and SIGTERM as container PID 1
feat(labs): check webapp image on every node before web runs
fix(labs): kubelet eviction reclaim 2Gi
feat(apps): add online-boutique for project experiments
docs: trim readme, details move to docs
chore: add commit-msg and pre-commit hooks
```

## Workflow

- Commit only when asked. The user often commits and pushes themselves.
- Work on a branch and open a pull request. Merge only with explicit permission:
  the user merges.
- Pull requests carry no "Generated with" line.
- Never push unless asked, and check `git remote -v` first: croft moved to the
  user's personal repository, and an old clone may still point at the
  organisation's.
- Never commit `.env`, datasets, binaries, generated reports or credentials.
- Check `git diff` before finishing.
