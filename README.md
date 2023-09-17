# Ultros

Ultros is a Final Fantasy XIV marketboard analysis tool that utilizes data sourced from Universalis. Blazingly fast and written in Rust.

Uses Axum, Leptos, SeaOrm, and Serenity to create something that works most the time!

Currently hosted with linode via https://ultros.app

### Development

Ultros requires a nightly rust toolchain which can be acquired with `rustup`. Check (rustup.rs)[https://rustup.rs] for more details.

This project utilizes git submodules to bring in assets. Since I'm not smart enough to put this into a build script, you must use `git submodule update --init` or `git clone --recursive` when cloning the project.

The project can be run with `cargo-leptos`, assuming a Rust toolchain is installed you can install it with `cargo install cargo-leptos --locked`. Then use `cargo leptos serve` or `cargo leptos watch`. Add `--release` to enable optimizations.

The application also requires a Postgresql database to operate and a Discord application token.

### Ads

Currently am experimenting with running ads on the site to see how much revenue can be generated. Ideally, I'd like to get the site hosting expenses
covered without trying to coerce community members into donating. The ads are entirely opt out via the settings page, and adblocks will continue working.

### Environment Variables
* `DISCORD_TOKEN` - A discord bot token
* `DATABASE_URL` - Postgres connection string
* `DISCORD_CLIENT_ID` - ID of your Discord application
* `DISCORD_CLIENT_SECRET` - Client secret of your Discord application
* `HOSTNAME` - Address that your app is hosted at. Necessary to get OAuth to work.
* `KEY` - A secret hash used to encrypt cookies
* `RUST_LOG` - Log level to log at. ex: `RUST_LOG=ultros=info,warn`

### Contributing

Contributing is always appreciated - this project is still just a hobby for me.
Feel free to open an issue, submit a PR, or contact me directly with feedback and changes requested.

### Crates

This repo has been my sandbox for FFXIV projects, and still has unused crates within the repo for reference, but might not compile or be maintained.

* `ultros` - Main crate for the ultros website. Contains main axum initialization and discord.
* `ultros-db` - Ultros' datastore. Uses SeaOrm to store data in Postgres.
* `migration` - SeaOrm migration executable to run executables.
* `xiv-gen` - Generates structs that represent ffxiv scraped data sourced from [https://github.com/xivapi/ffxiv-datamining](ffxiv-datamining), or rawexd from SaintCoinarch.
* `xiv-gen-db` - Statically embeds a compressed file containing xiv data.
* `ultros-api-types` - Common API types between the frontend and backend
* `universalis` - API wrapper for Universalis, contains a websocket API and a simple HTTP client using reqwest internally.
* `ultros-frontend` - Frontend crates for ultros, primarily driven by leptos.
    * `ultros-app` - Main leptos app code.
    * `ultros-client` - WASM Client for ultros-app. Basically just provides some context and an init function.
    * `ultros-xiv-icons` - An attempt to bundle assets using Rust build.rs files and then statically including them
