# Lists browser bundle budget

Tracks [#1512](https://github.com/akarras/ultros/issues/1512), under the
[#1510 promotion audit](https://github.com/akarras/ultros/issues/1510).
The budget applies to the growth of the **delivered, optimized WASM compressed
with gzip**, interpreted conservatively as **750,000 bytes**. It is not 750 KiB.

## Measurements

The historical pair uses Phase 4's direct base and the final reviewed PR #1400
head, not the earlier, confounded pair in `lists-sync.md`:

- Base: `591609c1241ad03529a1fb3a49f11a7d0255bab5`;
  tree `4ec493e27caf855d981687208023cb6f2f472a94`.
- Reviewed head: `59fbc662bbf07543b1fb81359a470b07675f0e67`;
  tree `6e2fc0e09b7e388da673c49a9d9d1a3c0fef1f0d`.
  The squash merge has the same tree but a different embedded commit stamp;
  these measurements built the reviewed head itself.

| Build | Raw WASM bytes | `gzip -n -9` bytes |
|---|---:|---:|
| Original direct base | 18,858,598 | 5,982,225 |
| Original reviewed head | 20,873,167 | 6,741,125 |
| Original growth | +2,014,569 | **+758,900** |
| Direct base, Loro codegen-units=1 | 18,858,598 | 5,982,225 |
| Reviewed head, Loro codegen-units=1 | 20,810,172 | 6,720,242 |
| Same-setting candidate growth | +1,951,574 | **+738,017** |
| Candidate saving versus original head | −62,995 | **−20,883** |
| Direct base, dedicated `wasm-release` profile | 18,858,606 | 5,982,172 |
| Reviewed head, dedicated `wasm-release` profile | 20,810,186 | 6,720,444 |
| Dedicated-profile growth | +1,951,580 | **+738,272** |

**The original controlled comparison failed by 8,900 bytes.** Preserve that
result. The candidate head completed the full frontend pipeline in 12m13s,
using a private copy of the same-revision Cargo cache and an empty site output.
The same-setting base completed in 4m44s under a 15-minute timeout, also with a
private same-revision cache and empty site output. Its raw and gzip hashes are
identical to the original base. The candidate pair is **11,983 bytes below the
budget**. This is evidence for the CLI compiler-setting candidate.

The dedicated-profile follow-up also passed the full frontend pipeline on both
historical revisions: 25m33s for the head, 23m16s for the base. Its verified delta
is **11,728 bytes below the budget**. The profile outputs are not byte-identical
to the CLI experiment (head gzip +202 bytes, base gzip −53 bytes); keep each
measurement separate. The original failure remains recorded. The final
integrated build and production soak are still pending.

SHA256 of the final delivered artifacts:

| Build | Raw SHA256 | Gzip SHA256 |
|---|---|---|
| Original base | `1a7523dd179c9979ba4434c426edb655205862894dc091313ea3b49144adf285` | `e4c3e49a577436223eff6571290935a1f2e85beb2b4cbd7e9b4353013ab15e48` |
| Candidate base (identical) | `1a7523dd179c9979ba4434c426edb655205862894dc091313ea3b49144adf285` | `e4c3e49a577436223eff6571290935a1f2e85beb2b4cbd7e9b4353013ab15e48` |
| Original head | `2d67d5ee4821ff694a169f2678210c3fc7fe304024ffab0a64c1e44b2e7ec5f6` | `23b3ef8edf57241e755001051758f99504856eafb743820b0effcf80701d0399` |
| Candidate head | `d0bdae4d2664566160f49540de1dcdb242e3944b7337380bb85d85c721c6be01` | `6e8c284d6d899bc688632a1c6ebd7ca7ce0cea71251c29c3b172da9c2f3c52ae` |
| Dedicated-profile base | `aecd253f208913f4c0ee463914892b2827fd28700e3df223fdadecf0c382f949` | `a4ec9de36d752e95e307235a5e410395380c4c9dc899a9c035269fb1c8d7e9e9` |
| Dedicated-profile head | `cf94b894c059c435b6795caebe1482474c9ef4dfba584b81fa5782daaecd6994` | `92939074c5817135eff6ed328377487c26da61c4344ed680afa0459a29de35cd` |

## Controlled reproduction

Use isolated, self-contained checkouts at the exact SHAs above with matching,
hydrated Git LFS data. Keep targets separate across revisions: build scripts
embed Git stamps and do not necessarily invalidate when only HEAD changes.
Record `git rev-parse HEAD HEAD^{tree}`, clean status before/after, `git lfs fsck`,
and SHA256 manifests of all `data/` files, Cargo.lock, Cargo.toml, rust-toolchain,
Dockerfile and `.cargo/config.toml`.

The measurements used Linux amd64, nightly **2026-01-06**, cargo-leptos **0.3.1**,
wasm-bindgen **0.2.126**, Binaryen **123**, Tailwind **4.1.10**, and GNU gzip
**1.12**. The locally retained frozen tooling image is
`sha256:f2f2c5ea422965171c4b2849dffa50db4e3689ffd79bd6d45519753c88adf98d`;
this is an image ID, not a published registry reference. Its original base was
`rust@sha256:9a73a5088750b4c95158ab26629c854c3d6fc4b173cb7bc8079ad252d8ed7bfa`.
Reprovisioned tooling must record binary hashes and OS package versions, and
must be identical for both builds. Do not compare a newly provisioned toolchain
with only one artifact from this table.

Run the historical command in each checkout, capturing logs and the exit code:

```sh
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 BINARYEN_CORES=1
export LEPTOS_OUTPUT_NAME=ultros WASM_BINDGEN_WEAKREF=1
export LEPTOS_WASM_OPT_VERSION=version_123 LEPTOS_TAILWIND_VERSION=v4.1.10
cargo leptos --manifest-path=./Cargo.toml build --release --frontend-only \
  --lib-cargo-args=--locked -vv
```

For the candidate experiment, add only
`--lib-cargo-args=--config=profile.release.package.loro-internal.codegen-units=1`
to **both** historical commands. On the head, verify the actual Loro rustc
invocation contains `-C opt-level=z -C codegen-units=1` and feature `counter`.
The successful experiment's verbose log confirms both repeated arguments
reached Cargo: `--no-default-features --locked
--config=profile.release.package.loro-internal.codegen-units=1 --release`.
Use 10 GiB memory, 3 CPU and one-job limits; the bounded head experiment had a
30-minute timeout. No test-auth feature or optimizer bypass is allowed.

Require the whole pipeline to pass, including bindgen, Binaryen
`-Oz --enable-bulk-memory --enable-nontrapping-float-to-int`, JS and CSS. Measure
only the final delivered artifact, never Cargo's pre-bindgen WASM:

```sh
gzip -n -9 -c target/site/pkg/ultros.wasm > ultros.wasm.gz
wc -c target/site/pkg/ultros.wasm ultros.wasm.gz
sha256sum target/site/pkg/ultros.wasm ultros.wasm.gz
gzip -dc ultros.wasm.gz | sha256sum
```

The initial base attempt used Tailwind 3 from a stale manifest key and failed
CSS. That attempt was preserved; the corrected full pipeline used Tailwind 4
for both revisions. The same-revision base retry's optimized WASM hash was
identical. Failed runs do not count as passing builds.

### Dedicated-profile follow-up

Apply only this change's Cargo.toml/Dockerfile diff to each exact historical
revision. The tested patch SHA256 is
`f3d9807e97c4a0f0bcd14ed0d901b1872ee3d83cd738177c17dfc1d0e8b20aab`.
The resulting configured trees were
`dc44822d4436d9df171437877d97df26014e6c64` (base) and
`c21d7d18038c85995616fe816023c216475fee98` (head). These are configuration-only
trees on the original commits, not new historical commits or the integrated
application. Both use the same frozen tooling and data described above.

Run without the diagnostic compiler override:

```sh
cargo leptos --manifest-path=./Cargo.toml build --release --frontend-only \
  --lib-cargo-args=--locked --lib-cargo-args=-vv -vv
```

Both logs show Cargo receiving `--no-default-features --locked -vv
--profile=wasm-release`. The head's actual Loro rustc invocation confirms
`-C opt-level=z -C codegen-units=1`, counter enabled, and output in the
`wasm-release` directory. Original release/server profile settings were checked
unchanged. Each run used a new private copy of its same-revision candidate cache
and an empty site directory; moving to the new profile directory invalidated
dependency outputs and triggered recompilation. The sequential runs stayed
within the same resource caps and a 45-minute timeout each. Both final artifacts
were rehashed and gzip-decompressed, original commit stamps verified, and
source/configuration, data, tools and commands checked against the records.

## Production configuration and remaining gates

The `wasm-release` profile inherits `release` and changes only Loro's package
codegen-units to 1. cargo-leptos selects it via `lib-profile-release`; Docker's
frontend `cargo chef cook` uses the same profile. The `server-release` profile
and its native memory controls are unchanged. Loro features, versions and CRDT
semantics are unchanged; counter support is required for quantities and undo.

The historical CLI experiment and dedicated-profile path are separately
measured above. Docker's chef profile alignment was checked statically; a full
Docker chef build is not claimed. Before shipping, run `check_ci.sh` and exercise
the release browser flows for convergence, offline recovery, compaction, schema
rejection and purchase undo. Browser validation must load the exact optimized
WASM whose hash is measured, with recorded matching SSR/site/runtime provenance;
debug WASM is not a substitute. Check memory/build-time behavior too; cache-seeded
timings do not measure clean compilation cost. Independently review the pair and
measure the final integrated Lists head in a fresh target after all feature
merges. These measurements do not complete the production-soak gate.
