# Ultros Discord

Owns Poise commands, gateway event handling, private setup menus, and bot
startup. The `ultros` binary supplies the shared services and starts the bot
with `start_discord`. This crate does not depend on the web application or
Leptos. The server injects its existing price-history renderer so Discord
and the website continue using the same chart implementation.

Run the bot's unit tests without credentials, a Discord connection, Postgres,
ClickHouse, or a running web server:

```bash
cargo test --locked -p ultros-discord
```

The retainer journey has three boundaries:

- `retainer_setup.rs` handles slash-command initialization, menu actions,
  validation, recovery, and reply construction. Its gateway adapter acknowledges
  interactions and collects component/modal events.
- `retainer_services.rs` defines the external effects and implements them using
  the invoking user's identity, the existing database, Lodestone, and event buses.
  Ownership checks remain in the real database service.
- `retainer_setup_tests.rs` uses fake services with recorded calls and injected
  failures. Tests run the same initialization, dispatcher, recovery, revision,
  and reply builders as the bot, including multi-character assignments,
  ambiguous searches, failed saves/assignments, stale controls, and removal.

The fake is limited to external effects; it does not reimplement command
decisions. Keep the real database regression tests in `ultros-db` too:
mock tests cannot prove SQL conflict handling or ownership enforcement.
Other command families currently retain their direct service calls and pure
helper tests; they can adopt this boundary as their flows need coverage.

`check_ci.sh` includes this crate through the workspace Clippy and test gates.
The `test-auth` feature forwards the existing market-isolation switch used by
the server's browser test fixtures. Gateway token/collector behavior still
requires Discord smoke testing.
