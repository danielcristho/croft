# Change Discipline

- Keep changes small and focused on the task.
- Do not change `.env`, credentials, datasets or the user's local files.
- Do not delete or overwrite existing changes in the worktree.
- Do not `git pull` or switch branches in the main checkout during a series. Use
  a worktree.
- Check `git diff` before finishing, and keep generated artifacts, binaries,
  datasets and secrets out of the change.
