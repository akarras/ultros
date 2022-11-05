# Ultros

Ultros is a Final Fantasy XIV marketboard analysis tool that utilizes data sourced from Universalis written in Rust.

### Development

Ultros requires a rust toolchain which can be acquired with `rustup`

The project can be run with `cargo run -p ultros`, or `cargo run -p ultros --release` to enable optimizations.

If you have need of it, you can also use `cargo run -p ultros --profile fastdev` to enable optimizations that don't take as long to compile.

### Crates

This repo has been my sandbox for FFXIV projects, and still has unused crates within the repo for reference, but might not compile or be maintained.

* `ultros` - Main crate for the ultros website
* `ultros-db` - Ultros' datastore
* `xiv-gen` - Generates structs that represent ffxiv scraped data sourced from [https://github.com/xivapi/ffxiv-datamining](ffxiv-datamining), or rawexd from SaintCoinarch.
* `xiv-gen-db` - Generates a simple bincode file that stores all the scraped data
* `universalis` - API wrapper for Universalis, contains a websocket API and a simple HTTP client using reqwest internally.
* `crafterstoolbox` - *Deprecated* My first attempt at a UI for marketboard data, written with egui.
* `xivapi` - *Deprecated* api wrapper for xivapi that I was using at one point, but no longer maintain
* `recipepricecheck` - *Deprecated* Attempt at using the Universalis API to calculate the price to craft an item using items sourced from multiple worlds.