# Browser startup data

These LFS-tracked Brotli q11 archives are projected from the full server packs
in `../xiv-db/`. `xiv_gen::browser::startup_data` defines the projection. Normal
`game-data-pack` generation writes both sets. To rebuild only the browser packs:

```sh
cargo run --release --locked -p game-data-pack --bin project-browser
```

The client loads `/static/startup/<content hash>/<locale>.rkyv` through the
browser HTTP cache, without an IndexedDB dependency. Offline device lists also
prepare this startup URL in their existing service-worker cache. Full server
packs remain on `/static/data/` for existing clients.

Descriptions load on opening a tooltip; NPC pages and their social metadata
use SSR resources so a direct load contains the correct data before hydration.
Client navigation fetches `/static/game-detail/<full-pack hash>/<locale>/<kind>/<id>`.
Successful details (including a missing record represented as JSON null) are
public and immutable. Stale versions return a non-cacheable conflict, not data
from a different revision under an immutable key. Languages are explicit in
both URLs; responses do not depend on cookies or browser language.

The wire structs are retained in this first projection: removed string/vector
payloads are empty and extraction-only numeric fields are zero. This saves the
bytes without changing the full server/generator schema. These fields are not
available to startup consumers. Future schema changes must run the audit and
both SSR and hydration builds; Rust's public-field warnings do not establish
that a field is unused.

Measured from the full packs in 081f770 (bytes):

| Locale | Full | Startup | Saved |
| --- | ---: | ---: | ---: |
| en | 2,746,297 | 1,718,599 | 1,027,698 |
| ja | 2,574,303 | 1,737,468 | 836,835 |
| de | 2,698,058 | 1,731,525 | 966,533 |
| fr | 2,618,394 | 1,739,671 | 878,723 |
| cn | 2,480,162 | 1,644,623 | 835,539 |
| ko | 2,551,349 | 1,721,151 | 830,198 |
| tc | 2,313,054 | 1,539,215 | 773,839 |

`game-data-pack`'s `pack_sanity` tests verify all seven generated projections,
NPC reference closure, and a startup size budget below 75% of each full pack.

The E2E driver also runs `integration/game-data-startup.cjs`. Against a server
from the same checkout, it verifies hydration with the old game-data IndexedDB
blocked, no full-pack download, hover-only descriptions in English/Japanese,
and client navigation plus direct SSR/hydration for an omitted NPC. Run it
alone with `BASE_URL=http://127.0.0.1:<port> npm --prefix integration run test:game-data-startup`.
