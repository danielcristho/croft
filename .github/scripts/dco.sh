#!/bin/sh
# dco.sh checks that every non-merge commit in $1..$2 is signed off by its author; bot commits are exempt.
set -eu
range="$1..$2"
failed=0
for commit in $(git rev-list --no-merges "$range"); do
  name=$(git show -s --format='%an' "$commit")
  email=$(git show -s --format='%ae' "$commit")
  case "$name" in *"[bot]") continue ;; esac
  if ! git show -s --format='%B' "$commit" | grep -qxF "Signed-off-by: $name <$email>"; then
    echo "✗ $(git show -s --format='%h %s' "$commit"): no 'Signed-off-by: $name <$email>'"
    failed=1
  fi
done
if [ "$failed" -ne 0 ]; then
  echo "Sign off with 'git commit -s'; for existing commits: git rebase --signoff $1"
  exit 1
fi
echo "✓ every commit is signed off"
