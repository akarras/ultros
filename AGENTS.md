# Agent Instructions

This repository enforces strict CI checks. Before committing any code, you **must** run the `check_ci.sh` script located in the root directory.

## Instructions

1.  **Run `./check_ci.sh`** after making changes.
2.  **Fix any errors** reported by the script.
    - If `cargo fmt` fails, run `cargo fmt --all` to fix formatting automatically.
    - If `cargo clippy` fails, address the warnings/errors in your code.
3.  **Do not commit** until `./check_ci.sh` passes successfully.

Failure to follow these steps will result in CI failures.

## Git hooks (optional but recommended)

Tracked hooks live under `scripts/hooks/`. One-time install:

```bash
./scripts/install-hooks.sh
```

This sets `core.hooksPath=scripts/hooks` (per-repo, not global) and gives you:

- **pre-commit** → `cargo fmt --all -- --check` (fast; catches the #1 CI failure)
- **pre-push** → `./check_ci.sh` (fmt + clippy)

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

### Caveats

- Requires a populated `.env` (DATABASE_URL, DISCORD_*, KEY) — or those vars exported directly.
- Windows: process-group cleanup is best-effort; if `cargo leptos serve` lingers, kill it manually.

### Optional: Glitchtip / Sentry error reporting

Set `GLITCHTIP_DSN` to a Glitchtip (or Sentry) DSN to ship panics + `error!` tracing events with backtraces. Unset → no-op, no network calls. The DSN itself contains the project key so no other env vars are needed. Set `RUST_BACKTRACE=1` in the container so spawned-task panics include a stack trace.

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
- **Git submodules**: Must be initialized (`git submodule update --init --recursive`) before building. Contains FFXIV game data CSVs and icon assets.
