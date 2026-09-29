# NPC vendor social card

**Date:** 2026-09-17
**Scope:** `/npc/:id` pages get their own evergreen share image and OG metadata, like items and analyzers already do.

## Goal

Sharing `https://ultros.app/npc/1001276?lang=en` today produces the generic homepage card. It should
instead show the NPC's name, how many items they sell, and where they stand.

## Card content

The existing 1200x630 template has five slots. They are filled as follows:

| Slot     | Content                                                                 |
| -------- | ----------------------------------------------------------------------- |
| Eyebrow  | `social_card_npc_eyebrow` — "FFXIV NPC VENDOR"                          |
| Title    | The NPC's `ENpcResident.singular` name                                  |
| Subtitle | `social_card_npc_subtitle` — "Sells {{ n }} items for gil"; when the NPC has no gil shop, `npc_no_items` |
| Footer   | The zone label of the first placement (`placement_label`, e.g. "Limsa Lominsa · Lower Decks"); when there is no placement, `npc_location_unknown` |
| Hero     | A 2x2 mosaic of up to four icons of items the NPC sells; a shopping-bag glyph when none |

OG description reuses the page's existing `npc_page_desc` key (name + zone). The `og:title` is the
NPC name followed by " · Ultros", as for every other card.

Item count is the same number the page shows: the sum of every shop's rows, including rows whose
item is missing from the regional pack. The mosaic picks the first four distinct item ids across the
NPC's shops (shop-id order, then row order) that resolve to a named item in the locale's pack.

## Identity and routing

- `SocialCardKind::Npc(i32)` parses from `/npc/<positive id>` and round-trips through
  `("npc", "<id>")` image parts, so the image URL is `/social/v2/{locale}/npc/{id}`.
- Unknown ids, ids without an `ENpcResident` row, and NPCs whose name is empty in the requested
  locale's pack resolve to `None` from `social_card_content`, so page metadata falls back to the
  localized homepage card and the image endpoint returns 404. This matches the regional-pack policy
  for items: never substitute an English name.
- The `?world=` query is rejected (400) for NPC cards, as for every non-item kind.

## Data flow

`social_card_content` already switches between `xiv_gen_db::data_for(language)` on the server and
`tracked_data()` in the browser. The NPC branch reuses two existing helpers so the card and the page
never disagree:

- `shops_for_npc` in `routes/npc_view.rs` (made `pub(crate)`) for the shop rows.
- `placement_label` from `ultros_ui_game::components::npc_locations` for the zone string.

`SocialCardHero` gains `Vendor([Option<i32>; 4])`, which stays `Copy`. The server maps it to a new
`CardHero::Vendor([Option<i32>; 4])` in `ultros-item-card`.

## Renderer

`draw_hero` handles `CardHero::Vendor`:

- Collect the icons that actually exist in the packed icon set.
- 0 icons: draw `BiShoppingBagRegular` at the usual hero spot, size 245.
- 1 icon: a single 208px icon at the usual spot, same as an item card.
- 2 icons: two 150px icons side by side, centred on the hero spot.
- 3 or 4 icons: a 2x2 grid of 136px icons with a 12px gap, centred on the hero spot; the fourth cell
  stays empty for three icons.

Rendering stays deterministic and does no IO beyond the packed icons.

## i18n

New keys in all seven locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with real translations:

- `social_card_npc_eyebrow`
- `social_card_npc_subtitle` (uses `{{ n }}`)

## Testing

- `social_card.rs`: `/npc/1001276` round-trips; `/npc/0`, `/npc/abc` map to Home; `from_parts("npc", "x")` is `None`.
- `social_card.rs` (ssr): every locale builds an NPC card for a known vendor with a non-empty title,
  subtitle containing the item count, footer equal to the page's zone label, and a hero holding at
  least one item id. A leve-only NPC (1000101) gets the no-shop subtitle and an all-`None` hero. A
  missing id returns `None`.
- `ultros-item-card`: renders the vendor hero for 0, 1, 2, 3 and 4 icons, including ids with no
  packed icon, and output is a valid 1200x630 PNG.
- `web/social_card.rs`: no change beyond the hero mapping; existing cache tests still apply.
