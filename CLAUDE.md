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

## Windows: OpenSSL via vendored build

`web-push` (Tier 3 of the notification work) pulls in `openssl` transitively via the `ece` crate. The `ultros` crate pins `openssl = { features = ["vendored"] }` so cargo compiles OpenSSL from source via `openssl-src` instead of needing a system library. This means **no `libssl-dev` / OpenSSL-dev-headers required** on Linux or Windows for `cargo build`.

Vendored builds need **Perl + a C compiler** to configure and build OpenSSL from source:

- **Linux**: `perl` is almost always present; if not, `apt install perl`. The CI image already has both.
- **Windows**: install [Strawberry Perl](https://strawberryperl.com/) (`winget install StrawberryPerl.StrawberryPerl`). Make sure `C:\Strawberry\perl\bin` is on PATH **before** Git's bundled MSYS Perl (`C:\Program Files\Git\usr\bin`) — the MSYS Perl is too minimal to run OpenSSL's `Configure` script and you'll get a `Locale::Maketext::Simple` error. From a fresh PowerShell:
  ```powershell
  $env:PATH = "C:\Strawberry\perl\bin;C:\Strawberry\c\bin;" + $env:PATH
  cargo build  # or ./check_ci.sh from Git Bash with the same PATH
  ```
  In Git Bash, prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `$PATH`.

The first build takes ~10 minutes (compiling OpenSSL from source); subsequent builds reuse the cached artifact.

## Optional: install git hooks

`./scripts/install-hooks.sh` wires `core.hooksPath` to `scripts/hooks/`. Pre-commit runs fmt-check (fast); pre-push runs the full `check_ci.sh`. Bypass with `--no-verify` if you must.

## E2E smoke

`./scripts/run_e2e.sh` brings up the app (or reuses one on `$BASE_URL`) and runs the Puppeteer screenshot harness in `integration/`. See AGENTS.md for details.

## Repo conventions

See `AGENTS.md` for the canonical agent instructions (services overview, env var gotchas, etc.). This file repeats the CI bit because it's the single most common failure mode for AI agents on this repo.
