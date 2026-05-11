# Claude Code instructions for Ultros

## Before committing — always

Run `./check_ci.sh` from the repo root. It runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. CI will fail on either, so fix anything it reports before committing.

- Formatting failures: `cargo fmt --all` to autofix.
- Clippy failures: read the warning, fix the code. Do not `#[allow]` to silence unless it's a genuine false-positive worth a comment.

## When the submodule isn't initialized

`./check_ci.sh` runs clippy which compiles the whole workspace, and the `xiv-gen-db` build script reads from `xiv-gen/ffxiv-datamining/` — a git submodule. The csv data for `cn`, `ko`, `tc` lives in *nested* submodules of `ffxiv-datamining` (separate xivapi-adjacent repos), so a non-recursive init only gets you en/ja/de/fr and the build still panics on `cn/Item.csv`.

Two paths:

1. Initialize **recursively**: `git submodule update --init --recursive` (use `--depth=1` to keep it fast). May require user permission depending on the sandbox.
2. If submodule init is blocked, **at least run `cargo fmt --all -- --check`** — it doesn't need the submodule and catches most CI failures from this repo's history. Note this in the PR so a reviewer knows clippy was not run.

Either way, *do not commit and push without running fmt-check* — every formatting mistake will fail CI and waste a round trip.

## Optional: install git hooks

`./scripts/install-hooks.sh` wires `core.hooksPath` to `scripts/hooks/`. Pre-commit runs fmt-check (fast); pre-push runs the full `check_ci.sh`. Bypass with `--no-verify` if you must.

## E2E smoke

`./scripts/run_e2e.sh` brings up the app (or reuses one on `$BASE_URL`) and runs the Puppeteer screenshot harness in `integration/`. See AGENTS.md for details.

## Repo conventions

See `AGENTS.md` for the canonical agent instructions (services overview, env var gotchas, etc.). This file repeats the CI bit because it's the single most common failure mode for AI agents on this repo.
