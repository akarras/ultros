# Listing hours

Show when listings for an item appear and when they get undercut, as an
hour-of-day heatmap. Two surfaces:

1. **Analyzer columns** (this change): two 24-cell strips per row, "Listing
   hours" and "Undercut hours", over the page's selected window, in the
   viewer's local time.
2. **Item page panel** (follow-up): a 7 × 24 hour-of-week grid for the one item
   on the page, with the world/datacenter and NQ/HQ toggles the undercut
   pressure pane already has.

## What a timestamp means

Nothing upstream knows when a listing was created.

- Dalamud uploads `LastReviewTime` as `DateTime.UtcNow` at parse time
  (`[Obsolete("Universalis Compatibility, contains a fake value")]` on
  `MarketBoardCurrentOfferings`). It is the moment the uploader's client read
  the board.
- Universalis overwrites `last_review_time` on every upload
  (`ON CONFLICT ... last_review_time = EXCLUDED.last_review_time`).
- Our write paths ignore review-time-only changes
  (`view_state_matches_model` compares price, quantity and HQ), so the
  `reviewed_at` on an `added` row is the upload that first showed the listing.

So a listing's creation lies somewhere between the previous observed upload of
its board and the first time it was seen. The strip bins the **first-seen**
time, `min(event_time, reviewed_at)`, and reports how many of those listings
were **pinned**: the previous observation of the same `(item, world)` board is
at most an hour earlier, so the hour bin is right to within an hour.

Uploads that change nothing emit no websocket event, so the previous
observation reads too early and the gap reads too wide. Pinned share errs low.

## Definitions

Per `(item, hq)`, across every world in the scope, over `[from, to)`:

- **New listing.** An `added` event, not from the seed (`source != 'snapshot'`),
  that is not the add half of a remove → add reprice (same listing identity,
  removal at most 600 s earlier, the same pairing `reprice_events_sql` uses).
- **First seen.** `min(event_time, reviewed_at)`; `reviewed_at` is ignored when
  it is not positive.
- **Pinned.** Some `listing_events` row for the same item and world, any
  quality, lies in `[first_seen - 3600, first_seen - 5)`. The 5 s margin keeps
  the listing's own upload batch from counting as its predecessor. The reducer
  reads from `from - 600`, so listings in the first hour of the window can only
  be pinned by the last ten minutes before it.
- **Undercut hour.** The hour of an undercut event, exactly as
  `reprice_events_sql` defines undercuts.

Hours are UTC hour-of-day (`unix % 86400 / 3600`). The client rotates them to
the viewer's zone; a half-hour zone rounds to the nearest whole hour.

## Wire shape

`ListingWindowStats` gains, all `#[serde(default)]` so stored snapshot
generations and older servers still decode:

| Field | Type | Meaning |
|---|---|---|
| `new_listings` | `u64` | New listings in the window |
| `new_listings_pinned` | `u64` | Of those, how many are pinned |
| `new_listing_hours` | `[u32; 24]` | New listings by UTC hour of first sight |
| `undercut_hours` | `[u32; 24]` | Undercuts by UTC hour |

The hour arrays are skipped when all zero. Everything is computed in the
existing `listing_history::window` reducer, one extra bounded query per item
batch, and stored in the existing snapshot generations. No new table.

## Analyzer columns

- `market-listing-hours` and `market-undercut-hours`, follow-window like the
  other listing columns.
- The cell draws 24 cells, shaded against that row's own busiest hour, so the
  strip shows the item's daily rhythm; the per-day columns show volume.
- The sort and filter value is the busiest local hour (0–23).
- A listing-hours strip whose pinned share is under half is drawn faded and
  says so on hover: for that item it mostly maps when players browse the board,
  not when sellers list.
- A row with nothing to bin shows a dash, never an empty strip.

## Not in this change

- The item page's 7 × 24 panel.
- "Undercuts this hour vs usual": needs a count fresher than the snapshot
  generation that feeds the grid.
