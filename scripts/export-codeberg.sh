#!/usr/bin/env bash
set -euo pipefail

remote="${FIGUREFIT_REMOTE:-ssh://git@codeberg.org/caniko/FigureFit.git}"
branch="${FIGUREFIT_BRANCH:-main}"
prefix="crates/tools/figurefit"
split_ref="figurefit-codeberg-split"

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

if ! git diff --quiet -- "$prefix"; then
    echo "FigureFit subtree has uncommitted changes. Commit them before exporting." >&2
    exit 1
fi

git branch -D "$split_ref" >/dev/null 2>&1 || true
git subtree split --prefix="$prefix" -b "$split_ref"
git push "$remote" "$split_ref:$branch"

echo "Pushed $prefix to $remote ($branch)."
