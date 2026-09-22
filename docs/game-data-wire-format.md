# Letting the browser decompress the game-data pack

Investigation of the idea in the "Let the browser decompress game data" issue: drop the
decompressor from the wasm bundle, label the pack with `Content-Encoding`, and let the
browser inflate it before `xiv-gen-db` deserializes.

**Short answer.** The idea works, but not for the reason it was proposed. Removing the
decoder saves **5,804 bytes** of the brotli-compressed wasm — 0.17%. What the change
actually unlocks is a **format** switch: once the browser owns decompression, the pack can
be brotli instead of zlib for free, which is **1.75 MB off the first-visit download** and
**11.7 MB off the server binary**. It also fixes a server that currently spends brotli
`q11` CPU re-compressing an already-compressed 4.5 MB blob to gain nine bytes.

Nothing here is shipped. This document records the measurements and the trade-offs so the
call can be made on numbers.

## What happens today

`game-data-pack` serializes each locale with rkyv, deflates it with `flate2` at
`Compression::best()`, and writes `data/xiv-db/<lang>.rkyv` (tracked in LFS). The server
embeds all seven packs with `include_bytes!` and hands the raw bytes out of
`get_xiv_data_bytes` (`ultros/src/web.rs:3419`) at `/static/data/{version}/{lang}`, with
no `Content-Encoding` and no `Cache-Control`.

The wasm client fetches that URL, and `xiv_gen_db::decompress_data`
(`xiv-gen-db/src/lib.rs:131`) inflates it with `flate2`/`miniz_oxide` before
`rkyv::from_bytes`. It then stores the *compressed* bytes in IndexedDB, and the service
worker precaches the same URL into CacheStorage for the offline guest-list shell
(`publicGuestAsset`, `ultros/static/service-worker.js:59`).

Pack sizes, and what they inflate to:

| locale | pack (zlib `-9`) | rkyv archive | ratio |
| --- | ---: | ---: | ---: |
| en | 4,469,320 | 17,036,884 | 3.81× |
| ja | 4,307,125 | 17,675,000 | 4.10× |
| de | 4,409,965 | 17,380,036 | 3.94× |
| fr | 4,345,504 | 17,482,040 | 4.02× |
| cn | 4,057,471 | 16,413,672 | 4.05× |
| ko | 4,202,718 | 16,891,712 | 4.02× |
| tc | 3,764,092 | 15,407,836 | 4.09× |
| **total** | **29,556,195** | **118,287,180** | |

(The `~50 MB` in the `decompress_data` comment is stale — `en` inflates to 17 MB.)

## Measurement 1 — what the decoder costs in the bundle

Built `ultros-client` three ways and ran each through the real pipeline: `wasm-bindgen
0.2.126 --target web`, then cargo-leptos's `wasm-opt -Oz --enable-bulk-memory
--enable-nontrapping-float-to-int`, then `brotli -q 11`. Baseline is `9b93be11`.

| client build | wasm (raw) | wasm (brotli) | Δ brotli |
| --- | ---: | ---: | ---: |
| today — `flate2`/`miniz_oxide` inflate in wasm | 16,453,292 | 3,335,997 | — |
| **no decoder — browser inflates** | **16,438,027** | **3,330,193** | **−5,804 (−0.17%)** |
| `brotli-decompressor` 5.0.3 in wasm | 16,609,026 | 3,387,379 | +51,382 (+1.54%) |

`twiggy top` on the pre-bindgen module agrees: every `miniz_oxide`, `flate2` and
`ZlibDecoder` symbol together is **14,440 bytes** before `wasm-opt` — 0.09% of the module.
The zlib decoder was never a meaningful part of the bundle.

The third row is the one that decides the design. Decoding brotli *inside* wasm would buy
the smaller pack too, but the Rust brotli decoder costs 57 KB of wire bundle more than
no decoder at all (51 KB more than today's zlib decoder), on every visit — ten times what
removing the zlib decoder saves. Browser-decode is the only way to get the smaller pack
without paying for a decoder.

## Measurement 2 — what the format costs on the wire

Recompressing the inflated archives with `brotli -q 11`:

| locale | zlib `-9` (today) | brotli `-q11` | Δ |
| --- | ---: | ---: | ---: |
| en | 4,469,320 | 2,717,805 | −1,751,515 (−39.2%) |
| ja | 4,307,125 | 2,567,708 | −1,739,417 (−40.4%) |
| de | 4,409,965 | 2,659,583 | −1,750,382 (−39.7%) |
| fr | 4,345,504 | 2,609,495 | −1,736,009 (−39.9%) |
| cn | 4,057,471 | 2,471,172 | −1,586,299 (−39.1%) |
| ko | 4,202,718 | 2,543,263 | −1,659,455 (−39.5%) |
| tc | 3,764,092 | 2,303,566 | −1,460,526 (−38.8%) |
| **total** | **29,556,195** | **17,872,592** | **−11,683,603 (−39.5%)** |

Two separate wins hide in that table:

- **First visit**: a visitor downloads exactly one pack. `en` drops from 4.47 MB to
  2.72 MB — **1.75 MB**, roughly 300× the 5.8 KB the decoder removal saves, and about half
  the size of the whole wasm bundle.
- **Server binary and image**: every binary that enables `xiv-gen-db/embed` (`ultros`,
  `ultros-clickhouse`, `ultros-item-card`) `include_bytes!`s all seven packs. Brotli packs
  take **11.7 MB less** out of the binary and the Docker layer.

zstd `-19` was measured for reference at 2,942,513 for `en` — worse than brotli `-q11`,
and `Content-Encoding: zstd` has no Safari support. Brotli is the right choice.

## Measurement 3 — the server is double-compressing right now

`get_xiv_data_bytes` returns `&'static [u8]`, which axum labels
`application/octet-stream`. The global `CompressionLayer` (`ultros/src/web.rs:3774`)
only excludes images and bodies under 256 bytes, so it compresses the pack — bytes that
are already deflated.

`brotli -q 11` over `data/xiv-db/en.rkyv` produces **4,469,329 bytes from 4,469,320** —
**nine bytes larger** than the input, for **14.8 s** of CPU on this machine.

That quality figure is not a guess. `CompressionLayer::new()` defaults to
`CompressionLevel::Default`, which maps to async-compression's `Level::Default`, which for
brotli takes `BrotliEncoderParams::default()` — `quality: 11` (`brotli-8.0.4`,
`src/enc/encode.rs:330`). tower-http streams the body and flushes per chunk, so its real
cost and ratio differ from the one-shot CLI number above; the note in `pkg_service`
(`ultros/src/leptos.rs:172`) about the layer producing 7.2 MB where `-q11`
gives 4.9 MB is that same effect. The order of magnitude is the point: the origin burns
brotli-`q11` work on every cold pack request and gains nothing.

Setting `Content-Encoding` on the response fixes this for free — tower-http only
compresses when the response does not already carry that header
(`tower-http-0.6.11/src/compression/future.rs:43`). This is the same one-line change the
browser-decode plan needs, and it is worth doing on its own merits.

Worth checking separately: `/static/data/...` has no `Cache-Control` and `.rkyv` is not in
Cloudflare's default-cacheable extension list, so the edge may not be absorbing these
requests at all.

## The blocker: the packs are not valid standalone zlib streams

`game-data-pack` compresses with `FlushCompress::Full`, not `Finish`
(`game-data-pack/src/db.rs:117`). `Full` flushes all pending output but does not write
the final block marker or the Adler-32 trailer. The committed packs are
therefore **truncated zlib streams**: all the data is present, but the stream never ends.

```
$ python3 -c "import zlib; d=open('data/xiv-db/en.rkyv','rb').read(); o=zlib.decompressobj(); \
  out=o.decompress(d)+o.flush(); print(len(out), o.eof)"
17036884 False
```

`flate2`'s `ZlibDecoder::read_to_end` tolerates this — it returns what it has when the
reader hits EOF — which is why nothing has ever noticed. A browser will not: an
unterminated `Content-Encoding` body is a decoding failure, not a short read.

**Any version of this change requires regenerating the packs**, even the one that keeps
zlib. That is a one-word fix in the generator plus a new set of LFS objects — about 18 MB
as brotli, about 30 MB if the format stays zlib.

## Options

| | wire (en) | wasm (brotli) | server binary | needs repack | needs HTTP negotiation |
| --- | ---: | ---: | ---: | --- | --- |
| **A** — today | 4.47 MB | 3,335,997 | — | — | — |
| **B** — label existing zlib bytes | 4.47 MB | 3,330,193 | — | yes (`Finish`) | yes |
| **C** — brotli pack, browser decodes | **2.72 MB** | **3,330,193** | **−11.7 MB** | yes (format) | yes |
| **D** — brotli pack, wasm decodes | 2.72 MB | 3,387,379 | −11.7 MB | yes (format) | no |

**B** is not worth doing alone: it costs a repack and a protocol change to save 5.8 KB.
**D** avoids the negotiation risk but pays 57 KB of bundle on every visit forever to save
1.75 MB once per version — and it leaves the double-compression bug in place unless that
is fixed separately. **C** is the only option where every number moves the right way.

## Option C — pros and cons

**Pros**

- 1.75 MB off the first-visit game-data download (−39%).
- 11.7 MB off every binary that embeds the packs, and off the Docker image.
- 5.8 KB off the wasm bundle; `flate2`/`miniz_oxide` leave the client dependency tree.
- Stops the origin brotli-`q11`-ing 4.5 MB per cold request for a nine-byte loss.
- Decompression moves from `miniz_oxide` in wasm to the browser's native brotli, off the
  main thread as part of the fetch.
- Peak wasm heap during load drops by the size of the compressed input (~4.5 MB today),
  because the inflate buffer never exists inside linear memory.

**Cons**

- **Client-side caches grow.** The client can only cache what it receives, which is now
  the 17 MB archive rather than the 2.7 MB pack — 6.3× for both the IndexedDB game-data
  store and the service worker's offline-guest CacheStorage entry. This is the real cost
  of the change and it needs a decision, not a default. The clean answer is to drop the
  IndexedDB cache and put `Cache-Control: public, max-age=31536000, immutable` on the
  version-keyed URL: the HTTP cache then stores the *compressed* response, which is
  strictly better than a 17 MB IndexedDB write, and it would also delete the rexie schema
  workaround written for GlitchTip #7391. The service worker entry has no such escape —
  `cache.put` stores a decoded body — so the offline shell would carry 17 MB.
- **Content negotiation becomes load-bearing.** A client that does not send
  `Accept-Encoding: br` must still get something it can read, so the handler needs an
  identity fallback that inflates server-side, plus `Vary: Accept-Encoding`. Serving
  pre-encoded bytes without checking the request header would break those clients
  outright.
- **Cloudflare sits in front** and has to pass `Content-Encoding: br` through on a
  non-`/pkg/` path without re-encoding. `/pkg/` already relies on this via `ServeDir`'s
  precompressed siblings, which is good evidence it works, but it is unverified for this
  route.
- **LFS churn**: seven new pack objects, ~18 MB.
- The server needs a brotli decoder for `embed`. That is native-only and free in bundle
  terms, but it is a new dependency in `xiv-gen-db`.
- `game-data-pack` gets slower: brotli `-q11` over the `en` archive takes 47.6 s versus a
  few seconds for zlib, times seven locales. Generation is manual and rare, so this is
  cheap, but it is not nothing.

## What shipping C involves

1. `game-data-pack`: brotli `-q11` instead of `flate2` `Full`, and regenerate all seven
   packs. `--skip-icons` suffices; the CSV pins do not need to move.
2. `xiv-gen-db`: split `decompress_data` into the decode step and a public
   `archive_to_data` that takes an inflated archive; put the decoder behind a `decompress`
   feature implied by `embed`, so the hydrate build links neither `flate2` nor `brotli`.
   Server-side decode switches to `brotli-decompressor`.
3. `ultros/src/web.rs`: `get_xiv_data_bytes` checks `Accept-Encoding`, returns the pack
   with `Content-Encoding: br` plus `Vary: Accept-Encoding` and an `immutable`
   `Cache-Control`, and inflates server-side for clients that do not accept brotli.
4. `ultros-client` / `ultros-frontend-core`: call the archive entry point; decide the
   IndexedDB question (recommendation: drop the store and lean on the HTTP cache).
5. `game-data-pack/tests/pack_sanity.rs` follows the new format.

## Not measured here

- **In-browser decode time.** Native brotli should beat `miniz_oxide` in wasm, and
  `brotli -d` of the `en` pack takes 0.1 s natively, but no browser timing was taken — the
  Puppeteer harness needs database fixtures that are not available on this machine.
- **Whether Cloudflare passes `Content-Encoding: br` through on this route.**
- **tower-http's actual streaming brotli cost** for the current double-compression, as
  opposed to the CLI figure quoted above.

## Reproducing the bundle numbers

```bash
cargo build --profile wasm-release -p ultros-client --lib \
  --target wasm32-unknown-unknown --no-default-features --features hydrate
wasm-bindgen --target web --out-dir /tmp/bg \
  target/wasm32-unknown-unknown/wasm-release/ultros_client.wasm
~/.cache/cargo-leptos/wasm-opt-version_123/*/binaryen-version_123/bin/wasm-opt \
  -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
  -o /tmp/opt.wasm /tmp/bg/ultros_client_bg.wasm
brotli -q 11 -f -k -o /tmp/opt.wasm.br /tmp/opt.wasm
```

The wasm-bindgen CLI must match the crate version in `Cargo.lock` (0.2.126).
