# Agent Instructions

This repository enforces strict CI checks. Before committing any code, you **must** run the `check_ci.sh` script located in the root directory.

## Instructions

1.  **Run `./check_ci.sh`** after making changes.
2.  **Fix any errors** reported by the script.
    - If `cargo fmt` fails, run `cargo fmt --all` to fix formatting automatically.
    - If `cargo clippy` fails, address the warnings/errors in your code.
3.  **Do not commit** until `./check_ci.sh` passes successfully.

Failure to follow these steps will result in CI failures.

Note on feature-gated code: `xiv-gen`'s `csv_to_rkyv` module is behind the
non-default `csv_to_rkyv` feature, so plain `cargo test -p xiv-gen` does not
exercise it. `check_ci.sh` explicitly lints and tests this feature and runs
workspace library/binary unit tests through `scripts/check_tests.sh`. The six
Universalis live API smoke tests and existing ignored database tests remain
separate from this deterministic gate. The browser-only `ultros-client` is
excluded from native tests to keep SSR and hydration features separate; all
`ultros-app` SSR tests still run. Validate the client with `cargo leptos build`.
GitHub Actions also runs the local
JavaScript regression tests and a dependency security audit.

## Shipping a user-visible feature? Add a changelog entry

When a change alters something a player would notice — a new tool, a new
filter, a redesigned page, a bug that was visibly broken and now isn't — add
a new `YYYY-MM-DD-description.json` file to `ultros-changelog/changes/`.
Each change gets its own file, even when several ship on the same day.
Follow `ultros-changelog/README.md`: choose a category (`features`,
`improvements`, or `bug_fixes`) and importance (`high`, `medium`, or `low`).
The crate's `build.rs` generates the static entry list; do not edit a shared
array or generated output. Write the blurb for a player ("Get pinged when an
item crosses the price you set"), not for a reviewer. Refactors, dependency
bumps, and CI work don't belong there.

## Git hooks (optional but recommended)

Tracked hooks live under `scripts/hooks/`. One-time install:

```bash
./scripts/install-hooks.sh
```

This sets `core.hooksPath=scripts/hooks` (per-repo, not global) and gives you:

- **pre-commit** → `cargo fmt --all -- --check` (fast; catches the #1 CI failure)
- **pre-push** → `./check_ci.sh` (fmt + clippy + Rust regression tests)

Bypass once with `--no-verify`. Uninstall via `git config --unset core.hooksPath`.

## E2E (Puppeteer)

`integration/` contains a Puppeteer harness. The runner ([integration/runner.cjs](integration/runner.cjs)) visits a curated route list at desktop and mobile breakpoints, screenshots them, asserts on title tags and body content, and fails if any page logs `console.error` or a `pageerror`. A separate [integration/login.cjs](integration/login.cjs) exercises the test-auth login flow end-to-end.

### Driver

```bash
./scripts/run_e2e.sh
```

Default behavior: pick a free port, `cargo leptos build`, spawn `cargo leptos serve` on that port (`PORT`, `LEPTOS_SITE_ADDR`, and `HOSTNAME` all set accordingly), poll `/` for readiness, run the Puppeteer suite against the spawned server, then tear it down. Screenshots in `integration/artifacts/`; server log at `/tmp/ultros-e2e-server.log`.

Knobs:

| Env | Effect |
|---|---|
| `REUSE_SERVER=1` | Don't spawn — reuse a server already on `$BASE_URL` (default `http://127.0.0.1:8080`). Faster, but tests whatever build is up. **Do not use in multi-worktree setups** unless you're sure of which branch the existing server is from. |
| `E2E_PORT=N` | Pin to a specific port instead of a random one. |
| `LEPTOS_FEATURES="test-auth"` | Build with the `test-auth` cargo feature; enables `/test/login` and triggers the login-flow test. |
| `SKIP_BUILD=1` | Skip `cargo leptos build` (assumes a previous build is fresh). |
| `STRICT_CONSOLE=0` | Suppress the console.error / pageerror failure mode. |
| `SKIP_ASSERTS=1` | Skip per-route content assertions (screenshot smoke only). |
| `CONSOLE_ALLOW="foo,bar"` | Extra substrings to allow-list in console errors. |

### test-auth feature

Compile-time gated route `GET /test/login?user_id=...&username=...` that mints a session cookie + cache entry + DB row without any Discord round-trip. Defined in [ultros/src/web/oauth.rs](ultros/src/web/oauth.rs) under `#[cfg(feature = "test-auth")]` and registered in [ultros/src/web.rs](ultros/src/web.rs) via the `test_auth_routes()` helper. Prod Docker builds don't pass `--features test-auth`, so the route literally isn't in the binary.

To exercise login flow locally:

```bash
LEPTOS_FEATURES=test-auth ./scripts/run_e2e.sh
```

### Targeted probes

The runner only asserts on titles and body substrings, which can't see values that
hydration silently drops. [integration/jobset-card-hydration.cjs](integration/jobset-card-hydration.cjs)
covers that seam for the gear-set cards on `/items/jobset/<JOB>`: it reads the NQ/HQ
totals after a direct (SSR + hydrate) load and again after a client-side navigation to
the same route, and fails if they disagree. It needs market data to be meaningful but
passes rather than failing when there is none, so it's safe against an empty dev DB.

```bash
BASE_URL=http://127.0.0.1:8080 npm --prefix integration run test:jobset-card-hydration
```

`JOBSET` (default `SAM`) and `WORLD` (default `Gilgamesh`) pick the route.

### Caveats

- Requires a populated `.env` (DATABASE_URL, DISCORD_*, KEY) — or those vars exported directly.
- Windows: process-group cleanup is best-effort; if `cargo leptos serve` lingers, kill it manually.

### Optional: Glitchtip / Sentry error reporting

Set `GLITCHTIP_DSN` to a Glitchtip (or Sentry) DSN to ship panics + `error!` tracing events with backtraces. Unset → no-op, no network calls. The DSN itself contains the project key so no other env vars are needed. Set `RUST_BACKTRACE=1` in the container so spawned-task panics include a stack trace.

### Optional: disable the Universalis websocket ingest

Set `ULTROS_DISABLE_UNIVERSALIS_WEBSOCKET=true` to start the server without subscribing to the Universalis market feed. Intended for QA/staging deploys that share one database and aren't exercising live market data: the websocket spawns a database write per inbound event, so turning it off drops the write churn several replicas otherwise pile onto that one Postgres.

**It is not a fix for connection-pool exhaustion.** The pool is sized by `POSTGRES_MAX_CONNECTIONS` (`ultros-db/src/lib.rs`), and a replica with the ingest off still opens connections up to that ceiling serving ordinary page traffic. Budget the ceiling as `(server max_connections − headroom) / instances you run at once`; this flag only makes those instances quieter, it does not cap them.

Accepts `1`/`true`/`yes`/`on` (case-insensitive) to disable; unset, empty, `0`/`false`/`no`/`off` keep the ingest running, which is what production does. Any other value is treated as "disable" and logs a warning.

With it set, the app still fetches worlds/datacenters from Universalis at startup (nothing renders without them) and still serves whatever listings and sales are already in the database — they simply stop updating, so `ultros_world_ingest_staleness_seconds` will climb. On-demand refreshes (the periodic catch-up sweep in `item_update_service`, and manual sweeps) are unaffected.

E2E is currently run locally only — not wired into GitHub Actions. Run `./scripts/run_e2e.sh` before merging anything that touches routing, hydration, or the analyzer service.

## Cursor Cloud specific instructions

### Services overview

| Service | How to run | Notes |
|---------|-----------|-------|
| PostgreSQL | `sudo docker start ultros-dev` (pre-provisioned container) | Required. Runs on port 5432. |
| Ultros web app | `HOSTNAME=http://localhost:8080 cargo leptos serve` | Serves on `http://localhost:8080`. Compiles both server binary and WASM client. |

### Gotchas

- **`HOSTNAME` env var conflict**: The system sets `HOSTNAME=cursor`. The app reads `HOSTNAME` for OAuth redirect URLs and `dotenvy` will NOT override existing env vars. You **must** set `HOSTNAME=http://localhost:8080` explicitly when running the app (or `export HOSTNAME=http://localhost:8080` before running).
- **`KEY` env var**: The cookie encryption key must be at least 64 characters. The `.env` file has a sufficiently long value.
- **Discord bot panic**: With dummy `DISCORD_TOKEN`, the Discord bot task will panic on startup. This is expected and does not crash the web server (it runs on a spawned task).
- **`check_ci.sh` vs WASM build**: CI (`cargo clippy --all-targets`) only checks with the default `ssr` feature. The WASM/hydrate client build (via `cargo leptos serve`) may surface additional compile errors in `#[cfg(not(feature = "ssr"))]` code that clippy misses.
- **First-run initialization**: On first boot the app applies DB migrations and fetches FFXIV world/datacenter data from Universalis. This requires internet access.
- **Game data (LFS packs)**: FFXIV game data and icon assets come from pre-generated packs under `data/`, tracked via Git LFS. Run `git lfs install && git lfs pull` before building; no submodule init needed.
