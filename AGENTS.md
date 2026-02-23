# Project Rules

## Release

- Releases are automated by GitHub Actions (`.github/workflows/release.yml`).
- Trigger: push to `main`. Tags are date-based (`v2025.02.22.1`), auto-incremented.
- Do NOT create releases manually with `gh release create` or `gh release upload`.
- To release: merge `dev` → `main`, push `main`. That's it.

## Branching

- Work on `dev`. Merge to `main` only when ready to release.
- Fast-forward merges preferred (`git merge dev --no-edit`).

## Testing

- Run `cargo test` before merging to `main`.
- All tests must pass. Do not skip or disable tests.

## Local Install

- For local installation, use `sh ./install.sh`.
- Do NOT use `cargo install --path .` unless explicitly requested.
