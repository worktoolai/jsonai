#!/bin/sh
set -e

MAIN="main"
DEV="dev"

CURRENT=$(git branch --show-current)
if [ "$CURRENT" != "$DEV" ]; then
  echo "Error: must be on '$DEV' branch (currently on '$CURRENT')" >&2
  exit 1
fi

echo "Running tests..."
cargo test

echo "Merging $DEV → $MAIN..."
git checkout "$MAIN"
git merge "$DEV" --no-edit
git push origin "$DEV" "$MAIN"
git checkout "$DEV"

echo "Done. GitHub Actions will create the release."
