# Dependency audit follow-up

The Rust workflow publishes the complete `cargo audit` JSON report and a job
summary on every code PR and main push. Findings are currently **reported, not
blocking**: the 2026-09-23 re-run has 6 vulnerability findings across three
locked package/version pairs, down from 10 across five in the 2026-09-04
baseline. No advisory IDs are suppressed. Missing or malformed reports fail the
summary step.

Current run: RustSec advisory database commit
`f7dc4b2860b29978f400fda0aab31cc4dbd21134` (2026-09-22), 1,109 locked
dependencies, `cargo-audit 0.22.0`. This is dependency triage, not proof that
each vulnerability is exploitable in Ultros. Re-run the audit for current
results before remediating.

### Resolved since the baseline

The reqwest 0.11 → 0.12 consolidation (#1547) removed the old outbound HTTP
stack from the graph entirely, taking two of the five baseline pairs with it:

- `h2 0.3.27` (`RUSTSEC-2026-0258`) — no `h2` of any version remains in
  `Cargo.lock`.
- `rustls-webpki 0.101.7` (`RUSTSEC-2026-0098`, `0099`, `0104`) — its only
  consumer, `rustls 0.21.12` via `reqwest 0.11.27`, is gone.

### Open findings

| Dependency | Advisory IDs | Verified dependency path and next work |
| --- | --- | --- |
| `rustls-webpki 0.102.8` | `RUSTSEC-2026-0049`, `0098`, `0099`, `0104` | `poise 0.6.2` → `serenity 0.12.5` → `tokio-tungstenite 0.21.0` → `rustls 0.22.4`. This is now the only old TLS stack left. Upgrade the Discord dependency chain and verify gateway reconnect/authentication. A patched `rustls-webpki 0.103.15` also exists in the graph, but does not fix consumers of the older version. Advisories concern name constraints and CRL parsing; certificate/CRL configuration determines reachability. |
| `rkyv 0.7.46` | `RUSTSEC-2026-0235` | Directly used by game-data packs and analyzer persistence. Patch is `>=0.8.17`; no compatible 0.7 patch is listed. Inventory archived Rc/Arc and unsized pointer fields and input trust boundaries. Plan an archive-format migration with regenerated packs (`data/xiv-db` and `data/xiv-startup`) and deliberate invalidation/migration of analyzer snapshots, rather than a lockfile-only update. Current checked deserialization sites read bundled game data (server, and the wasm client decoding startup packs fetched from the Ultros origin) and local analyzer snapshot files (`ultros-analyzer/src/analyzer_service.rs`). |
| `rsa 0.9.10` | `RUSTSEC-2023-0071` | `ultros-alerts` → `web-push 0.11.0` → `jwt-simple 0.12.17` → `superboring 0.1.14`. No patch is listed. Determine whether RSA private-key operations are reachable through the VAPID signing path; the presence of RSA in the graph alone does not establish that. Prefer removing unused cryptographic algorithms or upgrading/replacing the owning dependency after that analysis. |

The report also retains informational unmaintained/unsound/yanked warnings.
Review them separately; a clean vulnerability count does not mean those warnings
have been resolved. The 2026-09-23 run lists nine unmaintained crates
(`atomic-polyfill`, `bitmaps`, `derivative`, `im`, `paste`, `proc-macro-error2`,
`rustybuzz`, `sized-chunks`, `ttf-parser`) and three unsound ones (`im 15.1.0`,
`lru 0.16.4`, `sized-chunks 0.6.5`); its yanked-crate check could not reach the
registry, so yanked status was not re-verified. Once remediation or individually documented applicability
decisions are complete, remove `continue-on-error` from the audit step to make
the security check blocking.

Reproduce the graph inspection without compiling:

```bash
cargo audit --file Cargo.lock --json > audit-report.json
node scripts/summarize_audit.cjs audit-report.json
cargo tree --locked -i rustls-webpki@0.102.8
cargo tree --locked -i rkyv@0.7.46
cargo tree --locked -i rsa@0.9.10
```

## Regression gate coverage

`./check_ci.sh` runs the SSR attribute guard, formatting, default and
`csv_to_rkyv` Clippy checks, then `scripts/check_tests.sh`. That script runs
workspace library/binary unit tests, the `ultros-list-doc` CRDT convergence
integration tests, the Universalis offline tests, the explicit CSV feature test
invocation, the game-data pack sanity test, and the regression tests for the
vendored `reactive_graph` patch (`vendor/reactive_graph/ULTROS_PATCH.md`). Existing ignored DB
tests, ClickHouse service integration targets, and six live Universalis API
smoke tests are not part of this deterministic gate. Their source tests remain
available for explicit service-backed runs.

The browser-only `ultros-client` entry point is excluded from native tests. It
has no authored unit tests and unconditionally enables `hydrate` on `ultros-app`
and Leptos; selecting it alongside the server merges browser and SSR features,
causing native SSR tests to call unsupported browser APIs. Every `ultros-app`
test remains selected with its SSR feature configuration. Validate client
compilation separately with `cargo leptos build`, which builds the WASM target;
this native gate does not substitute for that browser build or browser E2E.

The test script defaults `CARGO_PROFILE_TEST_DEBUG=0` to reduce debug-symbol
generation and artifact size; callers can override it when debugging a test.
Debug assertions, overflow checks, and all selected tests remain enabled.
Actions limits Cargo to two build jobs to reduce simultaneous compiler memory
use. The native Windows full-debug build showed a large SSR compiler footprint;
the actual peak on the Linux hosted runner still needs measurement from CI.

The workflow also runs `node --test integration/*.test.cjs` using local fixtures.
These exercise JavaScript regression behavior, not hydrated market calculations.
Full seeded browser CI still needs an isolated Postgres fixture with deterministic
world and market data, startup that avoids live Universalis initialization, and
assertions that fail when expected seed prices are absent. An empty database
smoke run should not be advertised as recipe or hydration regression coverage.
