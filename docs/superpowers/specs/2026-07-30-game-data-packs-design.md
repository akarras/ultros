# Game-data handling: worktree fallback + LFS data packs

Date: 2026-07-30
Status: Phase 1 implemented; Phase 2 approved in principle, pending spec review

## Problem

The repo bundles game data via three git submodules totaling ~2.1GB of working
tree (`xiv-gen/ffxiv-datamining` 1.6GB + nested cn/ko/tc, `universalis-assets`
498MB, `classjob-icons` 28MB). Every linked git worktree had to re-initialize
them by hand (the `--reference` dance in CLAUDE.md), and the checkout is
wildly oversized for what builds actually consume.

### Measurements (2026-07-30, data version 7.55)

- `ffxiv-datamining`: builds read **31 sheets per language** (~48MB of 222MB
  per lang; 1181 of 1212 CSVs never touched). Output: 7 × `database_{lang}.rkyv`,
  ~4.6MB each after flate2.
- `universalis-assets`: all 17,208 `icon2x` PNGs resized to 3 sizes (60/40/25px)
  and encoded with the `image` crate's WebP encoder, which is **lossless-only**.
  Output: `images.tar.zst` = **130MB** (~178MB raw webp).
  Lossy libwebp on a 300-icon sample: **q75 = 15%, q85 = 19.6%, q90 = 23.7%**
  of the lossless bytes → a q85 pack lands around **~30MB**.
- `database_en.rkyv` composition (flate2-best bytes): `items` **2.88MB of
  4.74MB (61%)** — dominated by `description`, which IS used (item_view.rs);
  then `e_npc_bases` 0.57MB, `e_npc_residents` 0.54MB, `recipes` 0.33MB.
  Recompressing the same payload: flate2-best 4.74MB → **zstd-19 3.11MB
  (−34%)**. This file is downloaded by every first-time visitor
  (`/static/data/{version}/{lang}.rkyv`), so this is page-load weight, not
  just repo hygiene.
- Partial clone validated: `git fetch --depth=1 --filter=blob:none origin
  <pinned-sha>` against xivapi/ffxiv-datamining took **1.2s**; sparse-checkout
  of 5 specific CSVs (incl. Item + ENpcBase) took **6s**, `.git` = 11MB.
  GitHub permits arbitrary-SHA fetches, so a generator script needs neither
  submodules nor full clones.

## Phase 1 — worktree fallback (implemented in this branch)

`xiv-gen/src/csv_to_rkyv.rs` and `ultros-frontend/ultros-xiv-icons/build.rs`
resolve their data dirs through a fallback chain:

1. `FFXIV_DATAMINING_DIR` / `UNIVERSALIS_ASSETS_DIR` env override
   (`cargo:rerun-if-env-changed` wired in the consuming build scripts);
2. the local submodule, when populated (probe: `csv/{en,cn,tc}/Item.csv` +
   `csv/ko/csv/Item.csv`; for icons an `icon2x/` holding ≥10k files — the full
   set is ~17.2k, so an interrupted fetch falls back instead of shipping a
   truncated tarball);
3. the main git worktree's copy, discovered via `git worktree list --porcelain`
   — with a `cargo:warning` when the fallback engages and another when the
   worktree's pinned submodule SHA differs from what main has checked out
   (both build paths).

The worktree-discovery/pin-drift mechanics live once, in
`xiv-gen/src/worktree_fallback.rs`, `include!`d by both sides; its tests run
via `cargo test -p xiv-gen --features csv_to_rkyv`. Both consuming build
scripts register the *resolved* data dir with `cargo:rerun-if-changed`
(un-canonicalized — `\\?\` paths from Windows canonicalization confuse cargo),
so a datamining/assets bump re-runs them even when the data lives outside the
package under the fallback.

CI and Docker (`actions/checkout` with `submodules: recursive`) hit case 2 and
are unaffected. The env overrides double as the seam Phase 2's generator uses.

## Phase 2 — LFS data packs

### Layout

```
data/
  manifest.toml          # upstream repo URLs + pinned SHAs + pack metadata
  xiv-db/{en,ja,de,fr,cn,ko,tc}.rkyv   # zstd-compressed, LFS-tracked
  icons/images.tar.zst                  # LFS-tracked
```

Both artifacts are consumed today via `include_bytes!(OUT_DIR/...)`; they
switch to including the LFS files from the repo path directly. The xiv-gen-db
build script (CSV parse + rkyv serialize, per build) and the ultros-xiv-icons
build script (17k-image resize, minutes of CPU on cold builds) are deleted
from the build path entirely — they move into the generator. Dev machines and
CI never need the data submodules again; the three submodules are removed
(`classjob-icons`: vendor the ~1.7MB of `src/` font files the CSS actually
references — the other 26MB of PSDs/stickers/sprites are dead weight).

### Format changes bundled into the pack switch

- **rkyv container: flate2 → zstd-19.** 4.74MB → 3.11MB for en. Decompression
  lives solely in `xiv_gen_db::try_init` (the client passes raw fetched bytes),
  so the only code change is swapping the inflater — `ruzstd` (pure Rust)
  works on both wasm and server. IndexedDB caching of the payload is
  format-agnostic.
- **Icons: lossless → lossy WebP.** Default **q85** (~30MB pack vs 130MB)
  pending visual sign-off on the comparison page generated during this
  investigation; q90 (~37MB) is the fallback if q85 shows artifacts.

### Generator: a cargo bin, not a GitHub Action

`cargo run -p game-data-pack -- [--latest | --pinned]`, in-workspace so it
reuses `xiv_gen::csv_to_rkyv::read_data` and the icon-resize code verbatim:

1. Read `data/manifest.toml` (5 upstream repos: ffxiv-datamining + the cn/ko/tc
   csv repos + universalis-assets, each with URL + SHA; ko tracks the
   `refactor` branch upstream and nests its csvs one level deeper).
2. `--latest`: resolve each repo's remote HEAD (`git ls-remote`) and update the
   manifest. `--pinned`: use the recorded SHAs (reproducible rebuild).
3. Fetch into a per-user cache (`~/.cache/ultros-game-data/<repo>/`):
   `git fetch --depth=1 --filter=blob:none origin <sha>` + sparse-checkout of
   exactly the 31 needed CSVs per language (seconds, ~50MB/lang). Icons need
   the full `icon2x/` blob set (~500MB) but only re-fetch when the pinned SHA
   moves.
4. Point `FFXIV_DATAMINING_DIR`/`UNIVERSALIS_ASSETS_DIR` at the cache and run
   the pack build: read_data × 7 langs → rkyv → zstd; resize + lossy-encode
   icons → tar → zstd.
5. Write packs + manifest; report a summary (item count delta, sheet-schema
   drift, items whose icons are missing upstream — icons historically lag
   name data).

The generator is the only thing that ever touches upstream repos.

### Automation

The existing `update_game_data.yml` schedule swaps its submodule bump for
`cargo run -p game-data-pack -- --latest` and opens a PR containing the
manifest change + regenerated LFS packs, with the generator's summary as the
PR body. Icon-lag detection (new items with no icon upstream) is part of that
summary instead of tribal knowledge. Local runs are identical — the workflow
is just a scheduler.

### Git LFS: costs and mitigations

Chosen storage is LFS (decision 2026-07-30). Honest numbers:

- Pack set ≈ **55MB/version** after the format changes (7 × ~3.5MB rkyv +
  ~30MB icons) — vs ~165MB without them; the format work above is what makes
  LFS comfortable.
- GitHub free tier: 1GB LFS storage total (history accumulates ~35–55MB per
  game-data bump), 1GB/month bandwidth. **Every un-cached CI checkout with
  `lfs: true` downloads the full pack set**, so bandwidth is the binding
  constraint.
- Mitigations, in order: (1) CI caches LFS objects keyed on the manifest hash —
  note the repo already brushes the 10GB Actions cache quota (per-ref cargo
  caches — the historical `not_found` Docker-cache failures), so the LFS cache
  should be small and shared-key, not per-ref; (2) a $5/mo GitHub data pack buys
  50GB storage + bandwidth if the free tier chafes; (3) `git lfs prune` /
  periodic history cleanup for retired pack versions.

### Migration order

1. Land Phase 1 (this branch).
2. Vendor classjob-icons `src/`, delete that submodule (independent, trivial).
3. Add the generator bin + manifest; generate packs from the current pins;
   `.gitattributes` LFS tracking for `data/`.
4. Switch `xiv-gen-db` and `ultros-xiv-icons` consumption to the packs
   (include from repo path; zstd/ruzstd swap; q85 icons). CI stops needing
   `submodules: recursive`; Dockerfile gains `lfs`.
5. Rewrite `update_game_data.yml` around the generator; delete the
   `ffxiv-datamining` and `universalis-assets` submodules and the CLAUDE.md
   submodule-init section.

Each step ships independently; the fallback from Phase 1 keeps everything
working throughout.

## Open questions

- **Icon quality**: q85 vs q90 — judge the generated comparison page.
- **LFS budget**: accept the $5/mo data pack up front, or start free-tier and
  watch the bandwidth meter?
- Should the served `/static/data/` payloads adopt zstd in the same PR as the
  pack switch (client + server together), or ship packs first with flate2
  retained to keep the diff smaller?
