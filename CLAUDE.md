# Claude Code instructions for Ultros

## Before committing — always

Run `./check_ci.sh` from the repo root. It runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. CI will fail on either, so fix anything it reports before committing.

- Formatting failures: `cargo fmt --all` to autofix.
- Clippy failures: read the warning, fix the code. Do not `#[allow]` to silence unless it's a genuine false-positive worth a comment.

## Game data comes from LFS packs

There are **no git submodules** in this repo anymore. FFXIV game data (item/recipe tables, icons, etc.) lives in pre-generated packs committed under `data/` (`data/xiv-db`, `data/icons`, `data/manifest.toml`) and tracked via Git LFS. `xiv-gen-db` and `ultros-xiv-icons` read these packs directly at compile time — there's no build-time network fetch and no submodule to initialize.

- **Fresh clone**: run `git lfs install && git lfs pull` once. Without it, the `data/` files are LFS pointer text, not real content, and the build fails with an actionable error message rather than a cryptic panic.
- **Worktrees**: no setup needed — `git worktree add` checks out LFS content the same as a normal clone as long as `git lfs install` has been run once on the machine.
- **Regenerating packs**: `cargo run --release -p game-data-pack -- --pinned` rebuilds the packs from the pins already recorded in `data/manifest.toml` (reproducible, no version bump). Pass `--latest` instead to bump the pins to the newest upstream data and regenerate against that.
- **`data/manifest.toml`**: records exactly which upstream commit/release each pack was generated from — this is the source of truth for "what version of game data is this."
- **Updating game data**: done by hand (in practice, by an agent), not on a schedule. A game-data
  bump can break consumers — a renamed sheet or column shifts `xiv-gen`'s generated types — so the
  regeneration and the fallout need fixing in the same change. Run
  `cargo run --release -p game-data-pack -- --latest`, then `cargo check -p xiv-gen-db --features embed`
  and `cargo test -p game-data-pack`, and resolve whatever the bump broke before opening the PR.

If you genuinely can't get LFS content (e.g. fully offline), **at least run `cargo fmt --all -- --check`** — it doesn't need the packs and catches most CI failures from this repo's history. Note this in the PR so a reviewer knows clippy was not run.

Either way, *do not commit and push without running fmt-check* — every formatting mistake will fail CI and waste a round trip.

### Reading `check_ci.sh`'s exit code

Don't pipe it into `tail`/`grep` and read `$?` — that's the pipe's status and will report a false success. Redirect and check explicitly:

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Clippy can also be **OOM-killed** on a memory-constrained machine (exit `137`, `Killed: 9`), which is not a lint failure. Re-run with `cargo clippy --all-targets -j 2 -- -D warnings` to lower peak memory.

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

## No hardcoded user-facing strings

Every user-facing string in `ultros-frontend/ultros-app/` must go through `leptos-i18n`. No string literals like `"Alerts"` or `"Library"` inside `view!` — use `t!(i18n, key)` (or `t_string!(i18n, key)` for attribute values).

When you introduce a new string:

1. Add the key to **every** locale file in `ultros-frontend/ultros-app/locales/` (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`). Adding only `en.json` is not acceptable — the build warns on missing keys per locale and `leptos-i18n` won't compile without the key in every file.
2. Provide a real translation for each locale, not an English stub. If you genuinely can't translate, copy the English value and flag it in the PR so a native speaker can fix it — but the default is to translate.
3. Use `snake_case` keys; group related strings by feature prefix (`venture_analyzer_*`, `welcome_*`) when there are several.

This applies to labels, headings, button text, aria-labels, tooltips, placeholders, toast messages — anything a user reads. Console logs, error messages bubbled to the dev console, and developer-only tooltips are fine to leave in English.

## Repo conventions

See `AGENTS.md` for the canonical agent instructions (services overview, env var gotchas, etc.). This file repeats the CI bit because it's the single most common failure mode for AI agents on this repo.
