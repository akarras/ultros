# Ultros Service Decomposition & Brokerless Scaling Plan

> **Status:** Revised after independent architectural reviews (Phase 0 CDN `Cache-Control` prep lands alongside this document)
> **Target deployment:** Single-host Docker Compose today, with an explicit operational path to horizontal `web` scaling and full cloud migration if expansion-launch demand outgrows the host.
> **Design constraint:** No external message brokers or cache daemons (no Redis, Kafka, or RabbitMQ). Favor boring, durable primitives already in the stack (Postgres, ClickHouse, Cloudflare, existing reverse proxy) plus targeted Rust-native accelerations where profiling proves their value.

---

## 1. Production Baseline & Observed Bottlenecks

Measured on the current single-host production deployment (48-core CPU, 76 GiB RAM with ~58 GiB free, Postgres 18 with `max_connections=150`, ClickHouse 25.4 with 24 GiB limit, Watchtower rolling restarts, and Cloudflare in front):

| Metric / Component | Observed Value | Implication |
|---|---|---|
| Monolith container (`ultros`) | ~1.7 GiB / 8 GiB RAM, ~43% CPU, `POSTGRES_MAX_CONNECTIONS=100` | Claims 2/3 of Postgres's connection limit alone; cannot run 2 replicas without re-tuning. |
| HTTP traffic mix | ~13.5 req/s steady, 25.6 req/s 24h peak; **~42% bots** (6.2 req/s) | High leverage from Cloudflare caching on public cards & JSON APIs (and eventually canonical SSR once prerequisites are met). |
| Top route by volume | `/item/{world}/{id}` (~6.3 req/s, ~47% of requests) | Drives most SSR + market-read work. |
| Top server-time sink | `/social/v2/{locale}/{kind}/{key}` (0.171 s/s, p95 559ms) + `/itemcard/{world}/{id}` (0.059 s/s, p95 619ms) | Already 100% cookie- and geo-independent with `public, max-age=86400`; Cloudflare skips extensionless URLs by default. A single Cache Rule fixes this without a new service. |
| Worst tail latency | `/api/v1/market_pulse/{world}` p95 = **9.75s** | Hard-TTL misses in `HomeFeedCache` run a live Postgres `COUNT(*)` over `active_listing`. |
| Analyzer snapshot | **302.7 MB raw / 99 MB gzip**, **19.53s** write time | Consumes two-thirds of the 30s SIGTERM budget during deploys, causing avoidable downtime. |
| Live broadcast lag | `ultros_analyzer_bus_lagged_total{bus="history"}` = **1,342 dropped sales / 24h** | Because `AnalyzerService` currently owns the ClickHouse `SaleRow` writer ([analyzer_service.rs](../../ultros-analyzer/src/analyzer_service.rs)), lagged events drift ClickHouse from Postgres. |
| Universalis WS ingest | ~20 events/s steady, 33.6 events/s 24h peak (~300–500/s expected at expansion launch) | Lightweight in Rust; main launch risk is Postgres `active_listing` write pressure and ClickHouse insert bursts. |

---

## 2. Architecture Direction: Two Deployable Roles First, Local Fixes Before Splits

An initial draft proposed splitting the monolith immediately into six separate services (`edge`, `web`, `market`, `ingest`, `bot`, `media`) with custom `tmpfs` `mmap` blobs and a Pingora proxy. Critical review surfaced two key realities:

1. **Most of the measured pain points have small, in-process fixes** that ship in days with zero distributed-systems risk (moving `SaleRow` off the broadcast consumer, moving snapshots off SIGTERM, adding SWR to `HomeFeedCache`, adding Cloudflare Cache Rules for cards and public JSON, and tuning `POSTGRES_MAX_CONNECTIONS`).
2. **Splitting `ingest` from `analyzer` adds a cross-process event protocol (`rkyv` schema versioning, producer epochs, ring-buffer replay) without helping the primary scaling goal** — separating stateless frontend/SSR deploys and scaling from stateful Universalis WebSocket ingest.

### Recommended Target Topology: One Binary, Two Deployable Roles (`ULTROS_ROLES`)

Keep a **single compiled server binary** gated by `ULTROS_ROLES` (default `all` for local dev and E2E), using Postgres session advisory locks (extending the pattern already in [ultros/src/main.rs](../../ultros/src/main.rs)) to elect singleton ownership:

```text
  Cloudflare (Global Edge Cache + Tiered Cache + stale-if-error)
     │  • /pkg/*, /static/*
     │  • /social/v2/*, /itemcard/*  (Cache Rule 1)
     │  • Public market JSON GETs    (Cache Rule 2)
     ▼
  Existing Reverse Proxy (upstream failover, healthchecks, rolling drain)
     │
     ├──────────────────────────────┐
     ▼                              ▼
  ┌──────────────────────────┐   ┌──────────────────────────────────────┐
  │  ultros-web (2..N)       │   │  ultros-market (1 active, optional   │
  │  ULTROS_ROLES=web        │   │  standby via PG advisory lock)       │
  │  • Leptos SSR + /pkg     │   │  ULTROS_ROLES=ingest,market,bot      │
  │  • Tantivy Search        │──►│  • Universalis WS + UpdateService    │
  │  • User/List/Group/      │   │  • Global RefreshJobs                │
  │    Retainer/Plugin CRUD  │   │  • AnalyzerService (in-process!)     │
  │  • Client WS / SSE       │   │  • ClickHouse outbox + rollups       │
  │  • /social/*, /itemcard/*│   │  • Hot market read APIs + SWR caches │
  └────────────┬─────────────┘   │  • Discord bot + AlertManager/Outbox │
               │                 └──────────────────┬───────────────────┘
               │ PG LISTEN/NOTIFY (wake-ups only)   │
               ▼                                    ▼
       ┌──────────────────────────────────────────────────┐
       │        Postgres 18  +  ClickHouse 25.4           │
       └──────────────────────────────────────────────────┘
```

#### Why This Split Works

* **Zero cross-process market ingest bus:** Universalis WS, `UpdateService`, `AnalyzerService`, and ClickHouse projection stay in the **same `ultros-market` process** over bounded in-process Tokio channels — eliminating custom replay rings, producer epochs, and N/N-1 `rkyv` schema drift during Phase 1.
* **Independent frontend scaling & deploys:** `ultros-web` has no Universalis socket, no Discord gateway, no ClickHouse rollup scheduler, and no 300 MB analyzer warm-up. Rolling 2+ `ultros-web` replicas behind the existing reverse proxy decouples UI/SSR deploys from market ingest.
* **Narrow `MarketClient` boundary:** `ultros-web` reads market data and triggers manual refreshes through a typed `MarketClient` trait with two implementations:
  * **In-process** (`ULTROS_ROLES=all`): direct calls for local dev and E2E tests.
  * **Coarse HTTP/2** (`ULTROS_MARKET_URL=http://ultros-market:8081`): coarse batch endpoints (`cheapest`, `recentSales`, `best_deals`, `trends`, `sale_stats`, `listing_stats`, `price_series`, `market_pulse`, `refresh`) so SSR never fans out per-item round trips.
* **Split a 3rd role (`ultros-bot`) or dedicated `ultros-media` only when metrics justify it:** Keeping Discord + `AlertManager` + `outbox` in the single-owner worker avoids an extra container and RPC boundary today; because `ultros-discord` and `ultros-alerts` are already separate crates, peeling off `ULTROS_ROLES=bot` later is a config flip.

---

## 3. Durability, Consistency & Rollout Contracts (Required Before Multi-Replica)

### 3.1 Durable ClickHouse Projection (Fixes 1,342 Dropped Sales/Day)
Moving `SaleRow` out of `AnalyzerService`'s broadcast loop removes broadcast-lag loss, but a bounded in-memory `mpsc` channel still drops rows on crash or queue overflow, and `UpdateService` catch-up writes ([item_update_service.rs](../../ultros-ingest/src/item_update_service.rs)) must feed the exact same path as WebSocket writes.

* **Contract:** Record new sales (and listing/receipt payloads that Postgres cannot reconstruct later) in a **transactional Postgres outbox** (or durable monotonic watermark on `sale_history.id`) inside the committing ingest transaction, drained in batches into ClickHouse and acknowledged only after ClickHouse confirms the insert (`ReplacingMergeTree` deduplicates by Postgres sale ID).

### 3.2 Postgres `LISTEN / NOTIFY` as Wake-Ups Only (Never a Durable Queue)
For cross-replica coordination across `ultros-web` replicas and `ultros-market`/`bot` (`lists`, `list_docs`, `notifications`, `alerts`, `retainers`, `auth_revoke`):

* **Never treat `NOTIFY` as authoritative delivery:** Listeners miss notifications while disconnected, payloads cap at 8,000 bytes, identical payloads in a transaction collapse, and `NOTIFY` takes a global lock at commit.
* **Rules:**
  1. Register `LISTEN` **before** reading initial state; reconcile on every reconnect and on a periodic fallback timer.
  2. Emit compact IDs only (never full Loro CRDT updates or per-keystroke deltas); subscribers fetch durable state from Postgres on wake-up.
  3. Keep `PgListener` on a dedicated non-transactional session connection (an idle-in-transaction listener stalls Postgres notify queue cleanup).
  4. **Multi-replica logout revocation (`AuthUserCache`):** Back logout and account deletion with a durable Postgres revocation record or session epoch; use `NOTIFY ultros_auth_revoke` only to evict early, and **flush the local `AuthUserCache` on listener reconnect** while keeping a short TTL.

### 3.3 Migrations, Watchtower, and N / N-1 Compatibility
Because `env!("GIT_HASH")` is compiled into binaries and Rust Docker builds are not bit-reproducible by default, every push to `main` produces a new digest and Watchtower rolling restarts will always run **version N and version N-1 concurrently**.

* **Schema migrations:** Move `Migrator::up()` out of every replica's `UltrosDb::connect()` ([ultros-db/src/lib.rs](../../ultros-db/src/lib.rs)) behind a single-owner advisory lock (or dedicated pre-deploy migration step) and follow expand/contract compatibility so N-1 survives N's schema.
* **Versioned `/pkg/<git-hash>/` assets:** With multiple `ultros-web` replicas rolling (or any HTML caching), HTML rendered by replica A (N-1) can request `/pkg/<old-hash>/ultros.wasm` from replica B (N). Retain the previous N-1 `/pkg/` bundle on a shared volume (or serve `/pkg/` from an immutable object store / Cloudflare R2) before rolling multiple `web` replicas or enabling edge HTML caching.
* **Stateless plugin keys:** Migrate `./data/plugin-keys.json` to `ULTROS_PLUGIN_PAIRING_KEY` and `ULTROS_PLUGIN_IDENTITY_KEY` env vars (already supported in [plugin.rs](../../ultros/src/web/plugin.rs)) so `ultros-web` has zero local-disk state.
* **Postgres connection pool budgeting:** Measure active `sqlx` pool usage before sizing; leave explicit headroom for overlapping rolling replicas, `PgListener` connections, advisory locks, migrations, and ops queries inside Postgres's `max_connections=150`.

---

## 4. Cloudflare Caching Strategy & Phase 0 Rules

### 4.1 Cookie & Personalization Inventory
* **Server-set (`HttpOnly`, AEAD `PrivateCookieJar`):** `discord_auth`, plus transient OAuth/plugin flow cookies (`oauth_state`, `pkce_challenge`, `pkce_verifier`, `login_next`, `ultros_game_link`, `ultros_game_approval`).
* **Client-set (`document.cookie`, read during SSR via `Cookies::new()`):** `HOME_WORLD`, `PRICE_ZONE`, `i18n_pref_locale`, `theme_mode`, `theme_palette`, `side_nav_collapsed`, `HIDE_ADS`, `CRAFT_OPTIONS`, `CHANGELOG_SEEN_COOKIE`, `DALAMUD_COOKIE`, and `last_view`.
* **Geo & Locale headers:** `CF-IPCountry` (maps to `GuessedRegion`, auto-selects `ja`/`cn`/`ko` on first visit) and `Accept-Language`.

### 4.2 Safe Cloudflare Cache Rules (Ship in Phase 0)

> **Cloudflare `Cache-Control` nuance:** Cloudflare documents that `s-maxage` disables `stale-while-revalidate` at the edge. All Phase 0 public JSON responses therefore emit `public, max-age=<ttl>, stale-while-revalidate=<swr>, stale-if-error=600` (without `s-maxage`), and **every error (`WebError`/`ApiError`) and degraded `200` fallback** (`market_pulse` soft-fail, `best_deals`/`trends` during analyzer warm-up) emits `Cache-Control: no-store`.

1. **Rule 1 — Social & Item Cards (largest immediate CPU win):**
   * Match: `(starts_with(http.request.uri.path, "/social/v2/") or starts_with(http.request.uri.path, "/itemcard/"))`
   * Settings: *Eligible for cache*, *Edge TTL: Respect origin (`public, max-age=86400, stale-if-error=86400`)*, *Browser TTL: Respect origin*, *Ignore cookies*.
   * Enable **Cloudflare Tiered Cache** so PoP misses collapse before hitting origin.
2. **Rule 2 — Verified Cookie-Independent Public Market JSON GETs:**
   * Match: `http.request.method eq "GET"` and path starts with `/api/v1/cheapest/`, `/api/v1/sale_stats/`, `/api/v1/listing_stats/`, `/api/v1/price_series/`, `/api/v1/price_density/`, `/api/v1/floor_history/`, `/api/v1/undercut_pressure/`, `/api/v1/item_stats/`, `/api/v1/market_pulse/`, `/api/v1/market_heat/`, `/api/v1/movers/`, `/api/v1/recentSales/`, `/api/v1/listings/`, `/api/v1/best_deals/`, `/api/v1/trends/`, `/api/v1/changelog`, or `/api/v1/game-history`.
   * Settings: *Eligible for cache*, *Edge TTL: Respect origin*, *Browser TTL: Respect origin*, *Cache Key: include query string* (preserves `format`, `window`, `show_suspicious`, `from`, `to`, `bucket`, `group`, `hq`, `direction`, `limit`), *Ignore cookies*.
   * *(Never match `POST` batch routes like `/api/v1/sparklines/*`, `/api/v1/resale_quality/*`, or `/api/v1/floor_history/{world}`.)*

### 4.3 Why Shared SSR HTML Caching Is Deliberately Deferred Beyond Phase 0

All Leptos SSR HTML responses stay `Cache-Control: private, no-store` in Phase 0. Enabling Cloudflare HTML caching requires **both** edge and origin to agree on cache eligibility before lookup — otherwise Cloudflare serves a cached English/NA page to a cookieless `JP`/`CN`/`KR`/`FR`/`DE`/`EU` visitor before the origin guard ever runs, and cached HTML breaks WASM boot after a deploy when `/pkg/<old-hash>/` returns 404.

Prerequisites before turning on shared SSR HTML caching:
1. **Explicit canonical URLs or edge-side rule parity:** Only cache `/item/{world}/{id}` and `/recipe/{id}?world=...` when the Cloudflare rule itself bypasses on any `Cookie`, any `Authorization`, CJK `ip.src.country in {"JP" "CN" "KR"}`, and non-English `Accept-Language` (or cache HTML exclusively when `?lang=<code>` is explicit in the URL).
2. **Response-side safety gate:** Require `status == 200` and `!headers.contains_key(SET_COOKIE)` before emitting a public header.
3. **N-1 `/pkg/<git-hash>/` asset retention** across the HTML cache + `stale-while-revalidate` window.

---

## 5. Rust-Native Accelerations (Profile-Driven, Deferred Until Measured)

Once Phase 0 and the 2-role split are live, apply self-built Rust optimizations strictly where profiles show real wins (without adding Redis/Kafka):

1. **Pre-Compressed Wire Cache Slabs (`Arc<EncodedBody>`):** Store `{ raw: Bytes, br: OnceLock<Bytes>, zstd: OnceLock<Bytes>, gzip: OnceLock<Bytes> }` in `StatsCache`, `HomeFeedCache`, and a generation-cached `cheapest_per_world` response so serving 1.5 MB bulk JSON becomes a zero-copy `Bytes::clone()` without per-request `serde_json` or Brotli CPU.
2. **In-App Tower Origin Shield:** A small Tower middleware providing single-flight coalescing + stale-while-revalidate keyed by normalized URL for public-cacheable endpoints (avoids adding a separate Pingora proxy SPOF while achieving the same origin protection).
3. **Single-Host Shared Memory (`mmap`) — Only If HTTP/2 Serialization Shows Up in Profiles:**
   * *Option A (Immutable generation files):* `ultros-market` publishes immutable per-generation `rkyv` files + an atomic manifest with a compile-time schema hash; readers validate once per generation (not every 1–2s) and budget for old mappings held by in-flight requests.
   * *Option B (Append-only `mmap` segment log, Chronicle Queue / Aeron style):* If `ingest` ever splits from `market` later, a single-writer append-only `mmap` log (`[len][seq][epoch][kind][rkyv]`) survives ingest restarts, replays without a socket server, and extends multi-node via HTTP range requests.

---

## 6. Expansion-Launch Readiness & Cloud Escape Hatch

* **Honest assessment of hybrid cloud bursting:** Item-page SSR executes sequential Postgres and ClickHouse queries (`SsrApi`), so remote cloud `ultros-web` nodes reaching back over WireGuard to a single home/prod host would multiply WAN RTT on every page render.
* **Primary launch levers:**
  1. **Cloudflare hit rate + Tiered Cache + `stale-if-error`** on cards, public market JSON, and (once prerequisites land) canonical cookieless SSR.
  2. **Vertical headroom** on the 48-core / 76 GiB host once connection pooling, `HomeFeedCache` SWR, and card caching remove artificial bottlenecks.
  3. **Write-path protection under 10–20× Universalis bursts:** Dedicated ingest pool budget, bounded refresh concurrency, and serving last-good market data with visible age instead of failing hard.
  4. **Full-stack cloud lift-and-shift escape hatch:** Because the 2-role split uses standard HTTP/2 + Postgres + ClickHouse with env-var secrets and zero mandatory host-local IPC, the entire Compose stack can move onto cloud VMs (or split `web` across nodes alongside co-located DBs) with no application rewrite.

---

## 7. Phased Rollout Roadmap

* **Phase 0 — Cloudflare & `Cache-Control` Prep (This PR)**
  * Standardize `public, max-age=..., stale-while-revalidate=..., stale-if-error=600` (without `s-maxage`) on cookie-independent public market `GET` endpoints (`listings`, `cheapest`, `item_stats`, `market_heat`, `recentSales`, `best_deals`, `trends`, `sale_stats`, `listing_stats`, `cached_json`) and `stale-if-error=86400` on cards.
  * Emit `Cache-Control: no-store` on all `WebError`/`ApiError` responses and degraded `200` fallbacks (`market_pulse` soft-fail, `best_deals`/`trends` warm-up).
  * Keep SSR HTML `private, no-store`; activate Cloudflare Cache Rules 1 & 2 + Tiered Cache.
* **Phase 1A — High-Leverage Local Fixes (In-Process, Zero Distributed Risk)**
  * Move ClickHouse `SaleRow` writes off the analyzer broadcast onto a durable Postgres outbox/watermark covering both Universalis WS and `UpdateService` catch-up (fixes 1,342 dropped sales/day).
  * Move the 19.5s analyzer snapshot off SIGTERM (periodic background snapshot; skip shutdown write if a recent snapshot exists).
  * Upgrade `HomeFeedCache` to stale-while-revalidate and replace `market_pulse`'s Postgres `COUNT(*)` (fixes 9.75s p95 tail).
  * Add `Arc<EncodedBody>` pre-compressed caching for bulk market endpoints and measure/tune `POSTGRES_MAX_CONNECTIONS`.
* **Phase 1B — Two-Role Split (`ULTROS_ROLES=web` ×N vs `ULTROS_ROLES=ingest,market,bot` ×1)**
  * Introduce the typed `MarketClient` trait (in-process + coarse HTTP/2).
  * Move `Migrator::up()` behind a single-owner advisory lock; migrate `plugin-keys.json` to env vars; retain N-1 `/pkg/<git-hash>/` assets across rolling deploys.
  * Wire Postgres `LISTEN/NOTIFY` wake-ups + durable logout revocation for multi-replica `ultros-web`.
  * Deploy `ultros-web-a` + `ultros-web-b` + `ultros-market` behind the existing reverse proxy.
* **Phase 2 — Profile-Driven Scale-Out (Only If Telemetry Demands It)**
  * Optional 3rd role (`ULTROS_ROLES=bot`), canonical URL SSR edge caching (with full edge/origin parity + N-1 `/pkg/`), or `mmap` generation snapshots / append-only segment log if profiling shows HTTP/2 market-read overhead.
