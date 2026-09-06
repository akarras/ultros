# Ultros social card design references

Date: 2026-09-04.

## Design decision

The user approved `approved-item-template.png` as the shared design template,
specifically its violet glow and punchy headline font. The three additional
images apply that direction to other pages and were also accepted. These
references guide the shared Rust renderer in `ultros-item-card`.

## References

| Image | Page | Status |
| --- | --- | --- |
| [Item template](approved-item-template.png) | Item view | Approved visual template |
| [Samurai gear sets](samurai-gear-sets.png) | `/items/jobset/SAM` | Accepted application |
| [Currency exchange](currency-exchange.png) | `/currency-exchange` | Accepted application |
| [Homepage](homepage.png) | `/` and generic fallback | Accepted application |

## Shared visual rules

- Production canvas: 1200 by 630. Generated references are approximately this
  aspect ratio, not exact-size production exports.
- Near-black plum background, approximately `#100b14`.
- Broad, smooth violet glow behind the right-hand hero; avoid visible rings.
- Large, very heavy ivory headline, approximately `#f7f1e8`, with tight leading.
  Match the approved image's letter shapes and weight. The image does not
  identify an actual font. The implementation uses embedded Outfit Black and
  Regular for Latin text and regional Noto Sans CJK Bold/Regular faces for
  Japanese, Korean and Chinese. See `ultros-item-card/assets/FONTS.md` for
  pinned sources, licenses and subset regeneration.
- Purple Ultros wordmark at upper left; lavender FFXIV page category upper right.
- Left-aligned headline occupies roughly two-thirds of the composition.
- A single subdued supporting sentence, a fine footer divider, context at
  lower left and `ultros.app` at lower right.
- Approximately 60px safe margins; preserve footer and masthead alignment
  across cards. Let headline size and wrapping adapt to content.
- No nested icon frames, fake buttons, outer border or decorative data charts.
- Inspect at full size and at approximately 400px wide before accepting a render.

## Reference content

| Page | Heading | Supporting copy | Footer context |
| --- | --- | --- | --- |
| Item | Actual item name | Compare listings across worlds | Explicit URL market scope |
| Job gear | Samurai gear sets | Compare gear across worlds | Samurai |
| Currency tool | Currency Exchange | Find what your currency can buy | Currency tools |
| Recipe plan | Crafted item name | Plan materials. Compare buying and crafting. | Final Fantasy XIV |
| Item category | Localized category name | Compare listings across worlds | Final Fantasy XIV |
| Gear-set detail | Localized job gear sets | Item level and gear comparison | Localized job name |
| Home/fallback | Your next market board advantage. | Compare prices. Plan your purchases. | Final Fantasy XIV |

Generic cards should not infer a market scope from the viewer's cookies.
World-specific or currency-specific routes can supply stable identities from
their URL. Avoid a region footer on generic tool URLs.

## Artwork and provenance

All four concepts were made with the built-in ImageGen tool. Each new mock
received the approved item image as an actual reference and instructions to
preserve its composition, typography, palette and glow. The currency mock
also received `ultros/static/images/gil.png` as a source reference.

The job card's sword and homepage discovery symbol are illustrative concept
art. During renderer implementation, use the existing official job icon for
Samurai and an agreed source/library icon for market discovery. Keep actual
packed item icons unchanged. Generated icon details are not authoritative
game art and should not be reconstructed from the mockup.

## Cache and implementation

- Keep prices, rankings, profit estimates, timestamps and live indicators out
  of both artwork and social descriptions.
- One deterministic Rust layout draws text and glow over source assets.
  These complete mockups are design references, never production card images.
- Preserve the existing separation between evergreen social cards and the
  Discord bot's explicitly requested price-history chart.
- Images use `/social/v2/{locale}/{kind}/{key}`, with an explicit `world` query
  only for scoped items. Generic keys are `default`. Supported locale codes
  are `en`, `ja`, `de`, `fr`, `ko`, `cn`, and `tc`.
- Recipe plans use `recipe/{recipe-id}` and the result item's packed icon;
  categories use `category/{numeric-id}` and the discovery hero; gear-set details
  use `jobset-level/{JOB}-{item-level}` and the existing job glyph. Invalid or
  regionally unavailable entities fall back to the localized home preview.
  Legacy name-keyed category links retain the generic item-explorer preview.
  Recipe planner quantities, crafting choices and market filters do not alter
  the evergreen image or its cache identity.
- Shared page URLs carry `?lang=<locale>`; unlocalized URLs have deterministic
  English social metadata. Private/unknown routes get the public home preview.
- Server metadata and images use the same catalog-backed content model in
  `ultros-app/src/social_card.rs`. Item/job names use localized game data.
  If a regional game pack does not contain an item or job yet, the page uses
  that language's generic home preview on both the server and browser. Direct
  image requests for unavailable localized entities return 404.
- A bounded 16 MiB / 64-entry cache and two CPU render slots contain crawler
  bursts. Responses have content ETags and a one-day public cache lifetime.
  No image URL rotates with time or prices; existing external previews can
  persist after a page points at revised artwork.
- Image dimensions, MIME type, descriptive alt text and locale are emitted
  in the initial server response along with one authoritative social title,
  description, URL and image.
- Verify direct server-rendered metadata, long names, missing icons, supported
  locales, and readability at chat-preview scale.

## Review notes

The new mockups preserve the approved dark ground, headline prominence and
glow. Their generated typography and spacing have small differences; the
approved item template remains the common target. The homepage is a useful
three-line headline stress case. Exact generated font shapes and illustrative
icons are not authoritative assets; production uses the documented fonts and
existing item/job/icon-library sources.

Renderer previews: `cargo run -p ultros-item-card --example preview`.
Crawler regressions: `BASE_URL=http://127.0.0.1:<port> npm --prefix integration run test:social-cards`.
Set `SOCIAL_ARTIFACTS=1` to save returned images, and `SOCIAL_HYDRATION=1` to
also check SSR/hydration parity and switching language in a browser.
For concurrent local worktrees, set `METRICS_PORT` to another free port when
starting the app; its default remains `9091`, independent of the web port.

Catalog-backed route previews (no database): `cargo run -p ultros --example social_card_previews`.
Outputs for all available regional catalogs appear in `target/route-card-previews/`.
