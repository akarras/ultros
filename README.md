# Ultros

**Live market board analytics for Final Fantasy XIV** — find cross-world flips, price your crafts, and know the moment you're undercut. Free, on every data center, at **[ultros.app](https://ultros.app)**.

[![CI](https://github.com/akarras/ultros/actions/workflows/rust.yml/badge.svg)](https://github.com/akarras/ultros/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-join%20the%20server-5865F2?logo=discord&logoColor=white)](https://discord.gg/pgdq9nGUP2)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/)

Ultros is a hobby project built for fun by one person — full-stack Rust ([Axum](https://github.com/tokio-rs/axum) + [Leptos](https://github.com/leptos-rs/leptos)), fed by live [Universalis](https://universalis.app) data. It runs optional ads purely to cover hosting: you can turn them off in the settings, and ad blockers work fine too.

![Ultros home dashboard](docs/screenshots/home.png)

## What it does

- **[Flip Finder](https://ultros.app/flip-finder)** — scan every world on your data center for items you can buy cheap and resell on your home world, with profit/day, sale velocity, drift, and confidence scoring.
- **[Item pages](https://ultros.app/item/Sargatanas/50327)** — live cross-world listings, sale history charts, outlier-filtered "real price", and vendor/exchange source info for every marketable item.
- **Analyzers for every gil-maker** — [recipe crafting profits](https://ultros.app/recipe-analyzer) (ingredient cost vs. sale price, with subcraft recursion), [levequest turn-ins](https://ultros.app/leve-analyzer), [retainer ventures](https://ultros.app/venture-analyzer), [scrip spending](https://ultros.app/scrip-sources), [vendor resale](https://ultros.app/vendor-resale), and [Free Company crafting](https://ultros.app/fc-crafting-analyzer).
- **[Market Trends](https://ultros.app/trends)** — 24-hour movers, category heat, and rising/falling items per world.
- **[Currency Exchange](https://ultros.app/currency-exchange)** — the best marketable items to buy with tomestones, scrips, seals, and every other in-game currency.
- **[Lists](https://ultros.app/list)** — shareable shopping lists with cheapest-listing lookups; plan a craft or a glam and share it with your Free Company.
- **[Retainer tracking](https://ultros.app/retainers)** — track your listings and get told the moment you're undercut.
- **[Price alerts](https://ultros.app/alerts)** — per-item price thresholds delivered by Discord DM or webhook.
- **[Discord bot](https://ultros.app/bot)** — market lookups and undercut alerts right in your server.

Every feature has an in-app guide at [ultros.app/help](https://ultros.app/help).

| Flip Finder | Sale history |
| --- | --- |
| ![Flip Finder](docs/screenshots/flip-finder.png) | ![Item sale history](docs/screenshots/item.png) |

| Market Trends | Recipe Analyzer |
| --- | --- |
| ![Market Trends](docs/screenshots/trends.png) | ![Recipe Analyzer](docs/screenshots/recipe-analyzer.png) |

<details>
<summary>More screenshots</summary>

![Venture Analyzer](docs/screenshots/venture-analyzer.png)

![Currency Exchange](docs/screenshots/currency-exchange.png)

</details>

## Links

- **Website**: [ultros.app](https://ultros.app)
- **Discord**: [discord.gg/pgdq9nGUP2](https://discord.gg/pgdq9nGUP2)
- **Discord bot setup**: [ultros.app/bot](https://ultros.app/bot)
- **Help & guides**: [ultros.app/help](https://ultros.app/help)

## Roadmap

Bigger plans live in [`docs/`](docs/) — for example the [price alerts feature notes](docs/price-alerts.md). Have an idea? [Open an issue](https://github.com/akarras/ultros/issues) or drop by the Discord.

## Development

<details>
<summary>Prerequisites, running locally, environment variables, and project structure</summary>

The project is built using:
- **[Axum](https://github.com/tokio-rs/axum)**: Backend web framework
- **[Leptos](https://github.com/leptos-rs/leptos)**: Full-stack Rust web framework
- **[SeaORM](https://github.com/SeaQL/sea-orm)**: Async ORM for the database
- **[Serenity](https://github.com/serenity-rs/serenity)**: Discord bot library

### Prerequisites

*   **Rust Nightly Toolchain**: Ultros requires a nightly Rust toolchain. You can install it via [rustup.rs](https://rustup.rs).
*   **Git LFS**: Game data ships as LFS packs under `data/`. Clone normally, then run:
    ```bash
    git lfs install
    git lfs pull
    ```
    Without `git-lfs` installed, the build fails with an actionable error message.
*   **Postgres Database**: A running Postgres instance is required.
*   **cargo-leptos**: The build tool for Leptos apps. Install with:
    ```bash
    cargo install cargo-leptos --locked
    ```

### Running the Project

1.  **Database Setup**:
    We recommend using Docker to run a local Postgres instance:
    ```bash
    docker run --name ultros-dev -e POSTGRES_PASSWORD=ultros-dev-password -p 5432:5432 -d postgres
    ```

2.  **Environment Configuration**:
    Create a `.env` file in the repository root based on `.env.example`.

    **Minimal `.env` for local development:**
    ```env
    # Discord / OAuth (Required for login/bot features)
    DISCORD_TOKEN=your-token
    DISCORD_CLIENT_ID=your-client-id
    DISCORD_CLIENT_SECRET=your-client-secret
    HOSTNAME=http://localhost:8080
    KEY=some-random-secret-key-at-least-32-chars

    # Database
    # Note: Ensure username/password match your Docker container settings.
    DATABASE_URL=postgres://postgres:ultros-dev-password@localhost:5432/postgres

    # Server
    PORT=8080
    RUST_LOG=ultros=info,warn
    ```

3.  **Run the Application**:
    ```bash
    cargo leptos serve
    # Or for a release build with optimizations:
    cargo leptos serve --release
    ```

    *Note: On first boot, the app will apply database migrations and fetch game data (worlds, regions) from Universalis. A restart may be required after this initial fetch.*

### Updating Game Data

FFXIV data (item/recipe tables and item icons) ships as pre-generated packs under `data/`,
tracked with Git LFS and embedded at compile time — day-to-day development never fetches or
generates anything. When a game patch lands, the packs are regenerated with:

```bash
cargo run --release -p game-data-pack -- --latest
```

`--latest` bumps the pins in `data/manifest.toml` to the newest upstream data and rebuilds;
`--pinned` (the default) rebuilds reproducibly from the recorded pins. Commit the changed
packs and manifest together.

The two halves of the data come from different places:

*   **CSV packs** (`data/xiv-db/*.rkyv`) are built from the community
    [`ffxiv-datamining`](https://github.com/xivapi/ffxiv-datamining) CSV repos (all seven
    languages), fetched as sparse checkouts at the SHAs pinned in the manifest. This works on
    any machine with network access.
*   **The icon pack** (`data/icons/images.tar.zst`) is extracted directly from a **local FFXIV
    install**'s SqPack files (via the `icon-extract` crate) — no crawling, no assets repo. The
    generator auto-discovers the install in the standard Windows locations (SquareEnix default,
    Steam, Program Files (x86)); if yours lives elsewhere (another drive, XIVLauncher on
    Linux/Steam Deck), point at the directory that contains `game/`:

    ```bash
    cargo run --release -p game-data-pack -- --latest --game-path "D:/Games/FINAL FANTASY XIV Online"
    ```

    **Patch the game first.** The run reports how many named items have no icon in the install —
    a triple-digit count means your client is older than the pinned CSVs and the newest items'
    icons would be missing from the pack. The client version the icons came from is recorded
    under `[icons]` in `data/manifest.toml`.

    No FFXIV install on the machine? `--skip-icons` rebuilds only the CSV packs and leaves the
    committed icon pack untouched.

### Environment Variables

| Variable | Description | Default / Example |
| :--- | :--- | :--- |
| `DISCORD_TOKEN` | Discord Bot Token | Required |
| `DISCORD_CLIENT_ID` | Discord Application ID | Required |
| `DISCORD_CLIENT_SECRET` | Discord Client Secret | Required |
| `HOSTNAME` | Public URL of the app (for OAuth redirects) | `http://localhost:8080` |
| `ULTROS_INTERNAL_API_ORIGIN` | Origin the SSR renderer calls its own API on. Defaults to the loopback form of `LEPTOS_SITE_ADDR`, so the server never leaves the box to fetch its own data; only set this if the API lives somewhere else. | derived from `LEPTOS_SITE_ADDR` |
| `KEY` | Secret key for cookie encryption | Random string |
| `DATABASE_URL` | Postgres connection string | `postgres://user:pass@host/db` |
| `PORT` | HTTP server port | `8080` |
| `RUST_LOG` | Log filtering configuration | `ultros=info,warn` |
| `POSTGRES_MAX_CONNECTIONS`| Max DB connections | `50` |

### Project Structure

This repository contains several crates that make up the Ultros ecosystem:

*   **`ultros`**: The main backend crate. Initializes Axum, the Discord bot, and background services.
*   **`ultros-frontend`**: The frontend workspace.
    *   **`ultros-app`**: The main Leptos application code (shared between server and client).
    *   **`ultros-client`**: The WASM client entry point.
*   **`ultros-db`**: Database layer using SeaORM.
*   **`ultros-api-types`**: Shared types between frontend and backend.
*   **`universalis`**: A wrapper for the Universalis API (HTTP & WebSocket).
*   **`xiv-gen`**: Generates Rust structs from FFXIV game data (sourced from `ffxiv-datamining`).
*   **`xiv-gen-db`**: Statically embeds compressed game data for fast access.
*   **`game-data-pack`**: Regenerates the LFS-tracked data packs under `data/` (see
    [Updating Game Data](#updating-game-data)).
*   **`icon-extract`**: Reads item icons out of a local FFXIV install's SqPack files.
*   **`migration`**: Database migration tool.

See [`AGENTS.md`](AGENTS.md) and [`CLAUDE.md`](CLAUDE.md) for the full contributor workflow (CI checks, services overview, and environment gotchas).

</details>

## Contributing

Contributions are welcome! This project is a hobby, so it might be a bit messy in places. Feel free to open an issue, submit a PR, or contact me directly with feedback or feature requests.
