# Analyzer rebase validation — 2026-09-22

This report records new validation after rebasing the analyzer consolidation
onto `origin/main` at `ef1dda33`. Earlier results remain in
`analyzer-consistency-2026-09-21.md`; they are not counted as validation of the
rebased tree.

## Initial environment inspection

- Installed toolchain: `nightly-2026-09-20`, including rustfmt, Clippy, rust-src,
  and the `wasm32-unknown-unknown` target required by the new upstream pin.
  `rustc --version`: `1.100.0-nightly (feaadeeac 2026-09-19)`.
- Git Bash, Strawberry Perl, cargo-leptos 0.3.1, wasm-opt, Node, and Puppeteer
  25.10.0 are available. `npm --prefix integration ls --depth=0` passes.
- All 16 Git LFS assets are hydrated, including seven new `xiv-startup` packs,
  seven full `xiv-db` packs, icons, and maps.
- The prior `target/` directory is absent. Native and WASM validation require
  fresh compilation; old ignored build logs cannot be reused as evidence.
- Inspection found approximately 673 GB free disk and 20 GB free memory.
- No prior task-specific QA containers or listeners remain. Cached
  `postgres:17-alpine` and `clickhouse/clickhouse-server:24.8-alpine` images are
  available for new isolated loopback services.

## Required validation scope

The `AGENTS.md` and gate scripts were read directly from `origin/main` before
builds. Required checks remain root `./check_ci.sh`, separate client validation
through cargo-leptos, and relevant browser E2E. The upstream vendored
reactive-graph gate additionally includes `effect_disposed_first_run` alongside
the existing concurrent memo and asynchronous source-walk tests.

Upstream market requests now use `format=columnar` for cheapest listings,
recent sales, sale statistics, and listing statistics. Browser fixtures must
provide the new wire shape while preserving the existing row-value and
interaction assertions. Upstream also adds startup game-data packs and a
`wasm-symbols` tool for generated symbol maps; these are distinct from the
previous branch's validation environment.

The cold Windows validation uses command-only
`CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`, consistently across
CI and preview builds. This avoids the prior MSVC archive-size issue and reduces
debug artifact size. Assertions, overflow checks, features, and test selection
remain unchanged; repository Cargo profiles are not edited. Initial parallelism
is four compiler jobs, with resource pressure monitored.

## Validation results

- Root `./check_ci.sh` **passed** on the reconciled source: **2,640 tests
  passed** across **56 suite invocations**, **87 existing ignored**, and **6
  explicitly filtered live Universalis tests**. Counts include tests repeated
  by required commands. All-targets Clippy and explicit xiv-gen feature Clippy
  passed. Native app tests passed **666/666**, server tests **488/488**.
  Startup-pack projection/reference checks and the new
  `effect_disposed_first_run` gate passed. Log: `target/analyzer-rebase-ci.log`.
- `node --test integration/*.test.cjs`: **127 passed**, no failures, including
  the five columnar market-wire fixture tests. Log:
  `target/analyzer-rebase-js.log`.
- Fresh native preview and hydrated client builds: **pending**.
- Rebased browser checks: **pending**. Planned scope includes the relevant
  analyzer driver, populated columnar fixtures, Currency desktop/mobile,
  recommended/unrestricted/default interactions, and the four-case history
  matrix for fresh, remembered, and chosen-default `/items` entry.

Commands use the verified Git Bash and Strawberry Perl paths:

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:/usr/bin:$PATH"
export CARGO_BUILD_JOBS=4 OPENSSL_RUST_USE_NASM=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
./check_ci.sh
export CARGO_ENCODED_RUSTFLAGS=$(printf -- "--cfg=web_sys_unstable_apis\037--cfg=erase_components")
cargo leptos build --server-only --bin-features test-auth
unset CARGO_ENCODED_RUSTFLAGS
cargo leptos build --frontend-only --bin-features test-auth
```

The explicit native flags preserve the repository's two cfg values and avoid
cargo-leptos adding a redundant erase-components flag that invalidates the
native cache. The server's test-auth override also avoids optional jemalloc on
MSVC. Neither adjustment changes the native gate's test selection.
