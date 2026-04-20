#!/usr/bin/env bash
set -euo pipefail

# commit any pending changes
if [ -n "$(git status --porcelain)" ]; then
  read -rp "Commit message: " msg
  git add .
  git commit -m "$msg"
  git push
fi

# show the current (latest) tag, if any
current_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "none")
echo "Current tag: ${current_tag}"

# create and push tag
read -rp "Tag version (e.g. 1.0.0): " version
git tag "v${version}"
git push origin "v${version}"

echo "Pushed tag v${version}"
