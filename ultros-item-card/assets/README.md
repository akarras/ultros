# Embedded social-card assets

The renderer follows `docs/design/social-cards/approved-item-template.png`:
near-black plum, a smooth violet halo, large ivory Outfit Black text, a shared
masthead and footer, and one uncluttered hero. The generated design references
are never served as finished cards or sampled to reconstruct game artwork.

## Sources

- Item cards use `ultros_xiv_icons::get_item_image` with the existing packed
  FFXIV item artwork. Its native detail/resolution is preserved; scaling does
  not invent new artwork.
- Job cards use the bundled `ultros/static/classjob-icons/src/FFXIVAppIcons.ttf`
  and its original private-use glyphs. The existing XIVAPI MIT license is at
  `ultros/static/classjob-icons/LICENSE`.
- Currency cards use the site's existing `ultros/static/images/gil.png` with
  the exchange arrows from Boxicons.
- Search, analyzer, help, and exchange symbols come from the same Boxicons
  library used by the site's `icondata` dependency: `BiSearchAlt2Regular`,
  `BiBarChartAlt2Regular`, `BiHelpCircleRegular`, `BiTransferAltRegular`.
  `icondata_bi` supplies the SVG paths and the existing `resvg` renders them.
  Boxicons is MIT licensed; see <https://github.com/atisawd/boxicons>.
- Font source revisions, licenses, subsets, and regeneration instructions are
  documented in [FONTS.md](FONTS.md).

Missing item icons or unsupported job glyphs use the same search symbol over
the shared halo. No network requests or system fonts are needed at render time.

## Reproduce visual QA

Run `cargo run -p ultros-item-card --example preview`. Full 1200 × 630 PNGs
and 400 × 210 chat-preview images appear in `target/social-card-previews/`.
Pass another output directory as the first argument after `--` if needed.
The example includes all seven locales, long German names, all hero types, and
the item, Samurai, currency, and homepage design applications.

`cargo test -p ultros-item-card` checks every localized item/job name against
the embedded fonts, CJK wrapping, constrained long titles, missing artwork,
PNG dimensions, and deterministic rendering. Run it whenever the game packs,
translations, font subsets, or the renderer change.
