# Ultros

Ultros is a Final Fantasy XIV market board analysis tool that utilizes data sourced from Universalis. It is built with Rust for high performance and reliability.

The project is built using:
- **[Axum](https://github.com/tokio-rs/axum)**: Backend web framework
- **[Leptos](https://github.com/leptos-rs/leptos)**: Full-stack Rust web framework
- **[SeaORM](https://github.com/SeaQL/sea-orm)**: Async ORM for the database
- **[Serenity](https://github.com/serenity-rs/serenity)**: Discord bot library

The live version is hosted at [https://ultros.app](https://ultros.app).

A comprehensive user guide is available [here](https://book.ultros.app).

## Ads

The site currently runs ads to help cover hosting expenses without relying on donations. These ads are completely optional and can be disabled via the settings page. Ad blockers will also continue to work without issue.

## Development

### Prerequisites

*   **Rust Nightly Toolchain**: Ultros requires a nightly Rust toolchain. You can install it via [rustup.rs](https://rustup.rs).
*   **Git Submodules**: This project uses git submodules for assets.
    *   Clone with `git clone --recursive <repo_url>`
    *   Or update an existing checkout with `git submodule update --init`.
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

### Environment Variables

| Variable | Description | Default / Example |
| :--- | :--- | :--- |
| `DISCORD_TOKEN` | Discord Bot Token | Required |
| `DISCORD_CLIENT_ID` | Discord Application ID | Required |
| `DISCORD_CLIENT_SECRET` | Discord Client Secret | Required |
| `HOSTNAME` | Public URL of the app (for OAuth redirects) | `http://localhost:8080` |
| `KEY` | Secret key for cookie encryption | Random string |
| `DATABASE_URL` | Postgres connection string | `postgres://user:pass@host/db` |
| `PORT` | HTTP server port | `8080` |
| `RUST_LOG` | Log filtering configuration | `ultros=info,warn` |
| `POSTGRES_MAX_CONNECTIONS`| Max DB connections | `50` |

## Project Structure

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
*   **`migration`**: Database migration tool.

## Contributing

Contributions are welcome! This project is a hobby, so it might be a bit messy in places. Feel free to open an issue, submit a PR, or contact me directly with feedback or feature requests.
