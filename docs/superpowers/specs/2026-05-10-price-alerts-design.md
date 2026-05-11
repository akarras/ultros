# Price Alerts — Design

**Date:** 2026-05-10
**Status:** Approved for implementation planning
**Roadmap context:** This is the headline feature for the current quarter, driving the growth thesis (c) and the AI-leverage thesis (f). It composes with the in-progress Shared Lists work.

---

## Strategic context

This design sits inside a broader roadmap with three intentions:

- **(c) Growth — users / revenue.** Price Alerts drives daily return visits via Discord push, which converts to ad impressions on the items pages users land on. The AI suggestion surface is also a word-of-mouth moment.
- **(a) Ship a major feature this quarter.** Price Alerts is the major feature. Scope is bounded; phases are independently shippable.
- **(f) AI-leverage the project itself.** Two Claude-backed endpoints (item suggestions, threshold recommendations) replace work the user would otherwise do manually. Jules handles the repetitive Leptos form code; Claude handles the schema design, business logic, and AI integration.

---

## Target user

**Persona B — The Flipper.** Speed matters (minutes, not hours), noise tolerance is high, monitors many items, lives in Discord. Ultros's existing analyzer ecosystem (Flip Finder, Vendor Resale, Trends) already serves this persona; Alerts extends it from pull-mode to push-mode.

Crafters (Persona A) and casual collectors (Persona C) are secondary — they can use the same feature with different rule configurations, but UX is optimized for Persona B.

---

## Feature shape

Smart watchlists layered on top of existing Lists. A user:

1. Adds items to a List (existing flow)
2. Either configures alert conditions per item, or accepts AI-suggested defaults via a one-click "Suggest watches" flow
3. Receives notifications via Discord DM or a user-configured channel webhook
4. Reviews recent alert fires and manages rules from a new top-nav **Alerts** page

The user-defined `price_alert.rs` (currently a stub at [ultros/src/alerts/price_alert.rs](../../ultros/src/alerts/price_alert.rs)) is the integration point. The 369-line `undercut_alert.rs` provides the model for one of the three trigger types and proves the event-bus integration works.

---

## Trigger types (v1)

Three triggers, no more (YAGNI on "new listing" and "sale velocity" until users ask):

1. **Below threshold** — *"alert when any listing for item X drops below N gil on world/DC Y"*
2. **Undercut** — *"alert when the cheapest listing for item X is N gil or N% below my retainer's listing"* (reuses `undercut_alert.rs`)
3. **% drop from median** — *"alert when item X's cheapest listing is ≥M% below the 7-day rolling median"* (reuses outlier-filtering work from PR #497)

Each rule has one trigger type. Users can stack multiple rules per item.

---

## Data model

```sql
CREATE TABLE alert_rule (
    id              BIGSERIAL PRIMARY KEY,
    user_id         INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    list_item_id    BIGINT REFERENCES list_item(id) ON DELETE CASCADE,  -- nullable for ad-hoc rules
    item_id         INT NOT NULL,                                       -- denormalized for fast lookup
    world_selector  JSONB NOT NULL,                                     -- AnySelector serialized (world/DC/region)
    trigger_type    SMALLINT NOT NULL,                                  -- 0=below_threshold, 1=undercut, 2=pct_drop_median
    threshold_value INT NOT NULL,                                       -- gil amount, or basis points for %
    threshold_unit  SMALLINT NOT NULL,                                  -- 0=gil, 1=bps, 2=pct_of_median
    delivery_type   SMALLINT NOT NULL,                                  -- 0=discord_dm, 1=webhook
    webhook_url     TEXT,                                               -- non-null iff delivery_type=1
    cooldown_seconds INT NOT NULL DEFAULT 3600,
    last_fired_at   TIMESTAMPTZ,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_alert_rule_item_enabled ON alert_rule(item_id) WHERE enabled = TRUE;
CREATE INDEX idx_alert_rule_user ON alert_rule(user_id);

CREATE TABLE alert_event (
    id                 BIGSERIAL PRIMARY KEY,
    alert_rule_id      BIGINT NOT NULL REFERENCES alert_rule(id) ON DELETE CASCADE,
    fired_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    matched_listing_id BIGINT,
    matched_price      INT,
    delivered          BOOLEAN NOT NULL DEFAULT FALSE,
    delivery_error     TEXT
);
CREATE INDEX idx_alert_event_rule_fired ON alert_event(alert_rule_id, fired_at DESC);
```

**Rate limiting:**
- Per-rule cooldown: default 3600s (1 hour). User-configurable per rule.
- Per-user daily cap: 200 events. Hard limit, returns "alert capped" notice if exceeded.

---

## Delivery

**Discord DM** (default):
- Uses existing Serenity bot + the Discord user ID stored from OAuth login. No new auth flow.
- Payload: single message with embed:
  ```
  🎯 Eden Mode (HQ) dropped to 145,000 gil on Behemoth
  Median (7d): 180,000 · Δ −19%
  → ultros.app/item/24158
  ```
- Failure mode: if user has DMs disabled, log to `alert_event.delivery_error`, surface in-app on the Alerts page.

**Webhook** (opt-in per rule):
- User pastes a Discord webhook URL when creating/editing the rule.
- POST a Discord-embed-shaped payload to that URL.
- Validation at create time: HEAD the URL, confirm `discord.com/api/webhooks/` prefix.
- Composes with Shared Lists: a List shared to an FC can have rules whose webhooks go to the FC's Discord channel.

**Out of scope for v1:** email, web push, in-app native notifications (the Alerts page shows fires but doesn't push).

---

## AI surface (the f angle)

Two Claude-backed endpoints. Both optional; both cached aggressively.

### `POST /api/alerts/suggest-items`
**Input:** user_id (from session), optionally a target list_id.
**Process:** Server gathers user's analyzer view history + recent flip activity + existing Lists. Sends a structured prompt to Claude with this context.
**Output:** Claude returns 5-10 suggested items with:
- item_id, suggested trigger type, suggested threshold, one-line rationale
- e.g., *"Eden Mode — viewed 7x in past 14 days; median 180k, suggesting `below_threshold @ 150k`"*

**Cache:** Per-user, 24-hour TTL. The frontend has a "refresh suggestions" button to bust the cache, capped at 3 refreshes/day/user to prevent abuse.

### `POST /api/alerts/suggest-threshold/{item_id}`
**Input:** item_id, optional world_selector.
**Process:** Server pulls 7-day price history + recent median + IQR. Sends compact prompt to Claude: "for this item with this distribution, what's a sensible 'good deal' threshold?"
**Output:** suggested threshold + 1-sentence rationale.

**Cache:** Per-item, 1-hour TTL. No per-user limit (cheap, item-level).

**Cost projection:** Even with 1000 active users hitting suggest-items daily, cache hit rates push real Claude calls to under 100/day → trivial.

---

## Frontend surface

**Reuses existing List view:**
- Each list item row gains a small bell icon. Click → opens a side drawer with the trigger config (trigger type radio, threshold input, delivery picker).
- Per-list header gains "🔔 Alert defaults" — sets the defaults applied to all newly-added items.

**New top-nav entry: "Alerts":**
- Recent fires (last 50, paginated)
- Active rules table (sortable, filterable by list / item / status)
- "Suggest watches" button → calls the AI endpoint → review modal showing the 5-10 suggestions with per-item accept/skip
- Per-rule edit and disable/enable toggles

**Notifications inside the app:**
- Top-bar badge showing unread fires
- Clearable; resets on Alerts-page visit

---

## Build sequence

Each phase ships independently. Phase 1 ships a useful feature on its own (DMs only, manual rule creation).

### Phase 1 — Backend foundation (3-5 days, mix of Claude design + Jules implementation)

- Migration: `alert_rule` + `alert_event` tables
- Finish `price_alert.rs`: replace the stub `check_listings` with real per-trigger dispatch
- Wire to `EventReceivers` listings event stream (already exists)
- Discord DM delivery via Serenity bot
- API endpoints: `POST /api/alerts`, `GET /api/alerts`, `DELETE /api/alerts/{id}`, `PATCH /api/alerts/{id}`

### Phase 2 — Webhook delivery (1-2 days, mostly Jules-able)

- Webhook delivery dispatcher (HTTP POST with Discord-embed JSON)
- URL validation at create time
- Per-rule delivery_type wiring

### Phase 3 — Frontend (3-5 days, Leptos forms — heavy Jules use)

- Per-list-item bell icon + drawer (Leptos)
- Per-list "Alert defaults" config
- Top-nav "Alerts" page with rule manager + recent fires
- Top-bar badge for unread fires
- All wired to Phase 1 API

### Phase 4 — AI suggestions (2-3 days, Claude-specific)

- `POST /api/alerts/suggest-items` endpoint (server-side Claude API client + prompt + caching)
- `POST /api/alerts/suggest-threshold/{item_id}` endpoint (same pattern, smaller prompt)
- Frontend "Suggest watches" button + review modal
- Telemetry: log accept/reject rate to refine prompts later

---

## Risks & trade-offs

| Risk | Mitigation |
|---|---|
| Discord users disable DMs from non-friends → silent failures | Surface delivery errors on the Alerts page; webhook fallback option |
| Notification fatigue → users disable alerts entirely | Per-rule cooldown defaults to 1hr; daily cap at 200; UI prominently shows "received N today" |
| Coupling to Shared Lists work-in-progress slips Alerts | Rules can have `list_item_id = NULL` (ad-hoc). Shared Lists is a *first-class* integration but not a blocker. |
| Claude API cost spike from abuse | Per-user rate limit (3 suggest-items/day), per-item cache, server-side input length cap |
| Threshold suggestions are bad → users distrust the AI | v1 shows rationale; users edit before accepting; telemetry refines prompts |
| Event-bus throughput under load (10k listings/min) | The current `EventReceivers` already handles main listing ingestion volume; the trigger eval is in-memory hash lookups by item_id |

---

## Definition of done (v1)

- A new user can: create a watchlist, set a price-threshold alert on an item, receive a Discord DM when the threshold fires, and view the fire on the Alerts page within 5 minutes.
- A power user can: configure all three trigger types, set webhook delivery to an FC channel, manage 20+ rules, and use "Suggest watches" to add 10 items at once.
- The Discord bot processes ≥1000 alert events/day without falling behind the listings event stream.

---

## What's deferred (v1.5+ candidate features)

- **Email delivery** — needs transactional email provider, low priority for Persona B
- **Web push (PWA)** — broader reach but service-worker complexity, defer until Discord-only proves traction
- **Trigger types**: new-listing, sale-velocity, "back in stock," cross-world arbitrage detection
- **Paid tier**: e.g., real-time vs. 15-min-delayed alerts, or higher daily cap. Validate growth first.
- **Group alert sharing**: when a Shared List is shared, optionally clone the rules with permission gating
- **Smarter AI**: Claude reads your sales history to suggest *which items to flip* (not just which to watch). Cross-sell with the Flip Finder.

---

## How this connects to the broader roadmap

Beyond Price Alerts itself, the roadmap should cover:

### Stability bedrock (parallel track, ongoing)
- [#71](https://github.com/akarras/ultros/issues/71) Recipe Analyzer hard-lock on missing world param — small, high-value
- [#247](https://github.com/akarras/ultros/issues/247) UI unresponsive after update until ctrl+r — likely SW/cache issue, real product pain
- [#143](https://github.com/akarras/ultros/issues/143) gamedata cron action failure — silent data staleness risk
- [#45](https://github.com/akarras/ultros/issues/45) Sale-history stats bug with all-equal values

### Shared Lists finish-line
- [#481](https://github.com/akarras/ultros/issues/481) Shared Lists frontend implementation — alerts depend on this for the social-webhook angle to fully land

### AI automation infrastructure (the f angle, beyond Price Alerts)
- Wrap Claude API client as a shared crate (`ultros-claude`) used by Price Alerts AI endpoints and future features
- Establish telemetry pattern: log every AI suggestion + acceptance/rejection so prompts can iterate empirically
- Establish a Jules-PR review SOP: standing dedup rules, garbage-file rejection (clippy_out.txt, *.orig), `.Jules/` vs `.jules/` directory cleanup (Windows case-fold pain)

---

## Open questions deferred to implementation

- Exact Claude model choice (Sonnet 4.6 for cost, Opus 4.7 for harder cases?)
- Prompt structure for `suggest-items` — needs A/B exploration
- UI copy for "your alert fired" — should match Ultros's existing tone
- Whether to seed new accounts with a "try alerts" tutorial flow (probably yes, defer to UX iteration)

---

## Implementation status (2026-05-11)

- **Phase 1** (backend foundation) — shipped via PR #586
- **Phase 2** (webhook delivery) — combined into Phase 3 PR
- **Phase 3** (frontend UI: bell icon + drawer + /alerts page) — this PR

**Still deferred:**
- **Phase 4**: AI-suggested watchlist items + threshold recommendations
- **% drop from median** trigger (requires a median tracker reading from sale_history)
- **Live state refresh** in `PriceAlertTracker` (alerts created via API don't fire until app restart)
- **Per-list "alert defaults"** UI sugar
- **Top-bar badge** for unread fires
- **Email delivery**, **web push (PWA)** — out of original scope
