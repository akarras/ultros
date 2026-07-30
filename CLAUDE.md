# Claude Code instructions for Ultros

## Before committing — always

Run `./check_ci.sh` from the repo root. It runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. CI will fail on either, so fix anything it reports before committing.

- Formatting failures: `cargo fmt --all` to autofix.
- Clippy failures: read the warning, fix the code. Do not `#[allow]` to silence unless it's a genuine false-positive worth a comment.

## When the submodule isn't initialized

**Worktrees usually need no submodule setup.** The `xiv-gen-db` and `ultros-xiv-icons` build scripts resolve their data dirs with a fallback chain: `FFXIV_DATAMINING_DIR` / `UNIVERSALIS_ASSETS_DIR` env override → the local submodule if populated → **the main worktree's copy** (discovered via `git worktree list`). As long as the main checkout has its submodules populated, builds in a linked worktree just work; a `cargo:warning` tells you when the fallback (or a pin drift between the worktree's recorded submodule SHA and what main has checked out) is in play. `ultros/static/classjob-icons` is runtime-only static content — not needed to build.

The rest of this section is about populating the **main checkout** (or a standalone clone).

`./check_ci.sh` runs clippy which compiles the whole workspace, and the `xiv-gen-db` build script reads from `xiv-gen/ffxiv-datamining/` — a git submodule. The csv data for `cn`, `ko`, `tc` lives in *nested* submodules of `ffxiv-datamining` (separate xivapi-adjacent repos), so a non-recursive init only gets you en/ja/de/fr and the build still panics on `cn/Item.csv`.

### Use `--reference`, not `--init --recursive`

A plain `git submodule update --init --recursive` does **not** work reliably here, and `--depth=1` makes it worse. Three failure modes, all observed:

- **`universalis-assets` + `--depth=1`** — the shallow fetch doesn't contain the pinned commit, so git aborts with `fatal: Unable to find current revision in submodule path ...` and leaves the directory **empty**. `git submodule status` still shows it initialized, so it only surfaces later as `ultros-xiv-icons/build.rs` panicking with `No such file or directory` on `universalis-assets/icon2x`. A failed shallow attempt also leaves a broken per-worktree gitdir that makes retries fail until it's removed.
- **`ffxiv-datamining`** — a full clone from GitHub often dies partway with `RPC failed; curl 56 Recv failure: Connection reset by peer` / `fatal: early EOF`. Git retries once, then aborts the whole command.
- **Anything after the first failure** — because the abort is command-wide, later submodules in the same invocation get registered but never populated. `classjob-icons` checked out "successfully" at the right SHA with **zero files**, showing up in `git status` as wholesale deleted content.

Instead, initialize each submodule against the main clone's already-populated module dir. This is fast and mostly offline:

```bash
MAIN=/path/to/your/main/ffxiv-playground   # NOT the worktree

git submodule update --init --reference $MAIN/.git/modules/ultros-frontend/universalis-assets ultros-frontend/ultros-xiv-icons/universalis-assets
git submodule update --init --reference $MAIN/.git/modules/xiv-gen/ffxiv-datamining xiv-gen/ffxiv-datamining
git submodule update --init --force ultros/static/classjob-icons

# cn/ko/tc are NESTED submodules of ffxiv-datamining, also cached in main:
M=$MAIN/.git/modules/xiv-gen/ffxiv-datamining/modules/csv
for s in cn ko tc; do
  git -C xiv-gen/ffxiv-datamining submodule update --init --reference "$M/$s" "csv/$s"
done
```

Then **verify** rather than trusting exit codes — several of these fail silently:

```bash
ls xiv-gen/ffxiv-datamining/csv/{en,cn,tc}/Item.csv xiv-gen/ffxiv-datamining/csv/ko/csv/Item.csv
ls ultros-frontend/ultros-xiv-icons/universalis-assets/icon2x | head -1
ls ultros/static/classjob-icons | wc -l   # must be non-zero
git status --short                        # no submodule should show as modified
```

`csv/ko` genuinely nests one level deeper than its siblings (`csv/ko/csv/Item.csv`) — that's the ko repo's own layout, not a broken checkout.

If submodule init is blocked entirely, **at least run `cargo fmt --all -- --check`** — it doesn't need the submodule and catches most CI failures from this repo's history. Note this in the PR so a reviewer knows clippy was not run.

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
