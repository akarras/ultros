# Dependency audit follow-up

The Rust workflow publishes the complete `cargo audit` JSON report and a job
summary on every code PR and main push. Findings are currently **reported, not
blocking**: the 2026-09-04 baseline has 10 vulnerability findings across five
locked package/version pairs. No advisory IDs are suppressed. Missing or
malformed reports fail the summary step.

Baseline: RustSec advisory database commit
`5a0ebedfe8bdd2e295b171f4162f8c977bcad9a5` (2026-09-02), 1,075 locked dependencies.
This is dependency triage, not proof that each vulnerability is exploitable in
Ultros. Re-run the audit for current results before remediating.

| Dependency | Advisory IDs | Verified dependency path and next work |
| --- | --- | --- |
| `h2 0.3.27` | `RUSTSEC-2026-0258` | `reqwest 0.11.27` / `hyper 0.14.32`, used by Ultros, Universalis and the Lodestone client. Move all three consumers to an HTTP stack using patched `h2 >=0.4.16`; updating only Ultros leaves the older transitive client. The advisory concerns unbounded empty HTTP/2 DATA frames. This path is the outbound client stack, not evidence about the Axum listener. |
| `rustls-webpki 0.101.7` | `RUSTSEC-2026-0098`, `0099`, `0104` | `reqwest 0.11.27` → `rustls 0.21.12`. The same client migration must remove this old TLS stack. Advisories concern name constraints and CRL parsing; certificate/CRL configuration determines reachability. |
| `rustls-webpki 0.102.8` | `RUSTSEC-2026-0049`, `0098`, `0099`, `0104` | `poise 0.6.2` → `serenity 0.12.5` → `tokio-tungstenite 0.21.0` → `rustls 0.22.4`. Upgrade the Discord dependency chain and verify gateway reconnect/authentication. A patched `rustls-webpki 0.103.14` also exists in the graph, but does not fix consumers of these older versions. |
| `rkyv 0.7.46` | `RUSTSEC-2026-0235` | Directly used by game-data packs and analyzer persistence. Patch is `>=0.8.17`; no compatible 0.7 patch is listed. Inventory archived Rc/Arc and unsized pointer fields and input trust boundaries. Plan an archive-format migration with regenerated packs and deliberate invalidation/migration of analyzer snapshots, rather than a lockfile-only update. Current checked deserialization sites read bundled game data and local analyzer snapshot files. |
| `rsa 0.9.10` | `RUSTSEC-2023-0071` | `web-push 0.11.0` → `jwt-simple 0.12.17` → `superboring 0.1.14`. No patch is listed. Determine whether RSA private-key operations are reachable through the VAPID signing path; the presence of RSA in the graph alone does not establish that. Prefer removing unused cryptographic algorithms or upgrading/replacing the owning dependency after that analysis. |

The report also retains informational unmaintained/unsound/yanked warnings.
Review them separately; a clean vulnerability count does not mean those warnings
have been resolved. Once remediation or individually documented applicability
decisions are complete, remove `continue-on-error` from the audit step to make
the security check blocking.

Reproduce the graph inspection without compiling:

```bash
cargo audit --file Cargo.lock --json > audit-report.json
node scripts/summarize_audit.cjs audit-report.json
cargo tree --locked -i h2@0.3.27
cargo tree --locked -i rustls-webpki@0.101.7
cargo tree --locked -i rustls-webpki@0.102.8
cargo tree --locked -i rsa@0.9.10
```

## Regression gate coverage

`./check_ci.sh` runs the SSR attribute guard, formatting, default and
`csv_to_rkyv` Clippy checks, then `scripts/check_tests.sh`. That script runs
workspace library/binary unit tests, the Universalis offline tests, the explicit
CSV feature test invocation and game-data pack sanity test. Existing ignored DB
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
