#!/usr/bin/env bash
set -euo pipefail

# commit any pending changes
if [ -n "$(git status --porcelain)" ]; then
  read -rp "Commit message: " msg
  git add .
  git commit -m "$msg"
  git push
fi

# create and push tag
read -rp "Tag version (e.g. 1.0.0): " version
git tag "v${version}"
git push origin "v${version}"

echo "Pushed tag v${version}"
