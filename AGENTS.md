# Agent Instructions

This repository enforces strict CI checks. Before committing any code, you **must** run the `check_ci.sh` script located in the root directory.

## Instructions

1.  **Run `./check_ci.sh`** after making changes.
2.  **Fix any errors** reported by the script.
    - If `cargo fmt` fails, run `cargo fmt --all` to fix formatting automatically.
    - If `cargo clippy` fails, address the warnings/errors in your code.
3.  **Do not commit** until `./check_ci.sh` passes successfully.

Failure to follow these steps will result in CI failures.

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
