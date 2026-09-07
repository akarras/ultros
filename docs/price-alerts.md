# Price Alerts

Logged-in users can create per-item price-threshold alerts via the UI:
1. Add an item to a List
2. Click the bell icon on the item row
3. Pick a world/DC, set a threshold, choose Discord DM or webhook delivery
4. Manage rules + view recent fires at `/alerts`

API: `GET/POST /api/v1/alerts`, `PATCH/DELETE /api/v1/alerts/{id}`, `GET /api/v1/alerts/events`.

Delivery methods:
- Discord DM (default — uses your Discord OAuth identity)
- Discord channel webhook (paste a webhook URL from a channel's Integrations settings)

See `docs/superpowers/plans/2026-05-11-price-alerts-phase-2-3.md` for the Phase 2+3 implementation plan.

(Phase 4 — AI-suggested alert thresholds — is tracked separately.)

## Retainer sale alerts

Fires when one of your claimed retainers' listings disappears and a sale with
the same world, item, quality, price and quantity is recorded within five
minutes. Sales are inferred (Universalis never says whose listing sold), so
the matcher prefers to miss a sale over reporting a wrong one: if another
seller had a listing at the same price and quantity removed in the same
window, nothing fires. Create one from the bell on `/retainers/listings`, the
Alerts page drawer, or `/ffxiv retainer add_sale_alert` in Discord.
Design: `docs/superpowers/specs/2026-09-07-retainer-sold-alert-design.md`.
