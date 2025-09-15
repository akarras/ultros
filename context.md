# UI Theming/Polish — Current Status, Completed Work, and Next Steps

This document replaces prior context and reflects the latest work on light/dark theming, brand palette adoption, UI polish, and navigation structure. It serves as the single source of truth for what’s done and what remains.

---

## TL;DR

- Theme is standardized across major routes with tokenized colors and utilities (panel/card/btn/nav-link/input).
- Currency Exchange now has labeled quick filters, inline min/max controls, and visible active chips.
- Item Explorer rows reworked into a responsive grid with stronger light‑mode labels and a non‑wrapping “Add to List.”
- Nav right cluster unified (Login/Settings/Logout/Invite look consistent).
- Search results contrast improved; fuzzy search optimized for speed.
- Item View charts update instantly when theme/palette changes.
- History/Footer tokenized for light mode.
- Analyzer sort works as a replacement (no duplicated query keys), chips readable in light mode.

Focus next: finish Currency Exchange UX (clear-all, table header polish), finalize Item Explorer layout for mobile and sidebar perf, and complete small contrast/perf refinements for Search & Nav.

---

## What’s Implemented (Latest Batch)

1) Currency Exchange (UX clarity and light-mode visibility)
- Labeled quick filters:
  - “Price/item”, “Qty recv”, “Profit”, “Hours/sale”
- Inline min/max controls next to each label (desktop-friendly), with the original filter modal still available via icon.
- Active filter chips shown below the quick bar; each chip has a clear (x) action.
- Improved control visibility in light mode via token text, outline, and brand-ring tints.
- Primary input label clarified:
  - From: “Amount to exchange”
  - To: “How many of this currency do you have?”
- Sorting buttons previously tokenized to improve readability in light mode.

2) Item Explorer (readability + layout)
- Results reflowed to a responsive grid for desktop:
  - Fixed column spans ensure the “Add to List” button never wraps onto its own line.
  - Gaps and alignment tightened to reduce redundant whitespace.
- Labels strengthened in light mode:
  - Sort/direction controls (“ADDED/PRICE/NAME/ILVL/ASC/DESC”) use muted text for idle and brand-fg + underline for active.
- Sidebar menu buttons compacted; button text improved; overall visibility aligned to token colors.

3) Navigation — Right cluster uniformity
- All actionable items (Login/Settings/Logout/Invite) are consistent with nav-link styling and spacing, improving appearance on small viewports.

4) Search
- Improved readability in light mode:
  - Category text and iLvl use token text instead of overly-muted colors.
  - Hover/active tones slightly softened for clarity.
- Performance optimized:
  - Fast paths for very short queries (substring match).
  - Prefilter candidates by substring before doing fuzzy scoring.
  - Results capped to top 100.
  - Leaner scoring profile.

5) Item View
- Chart now redraws automatically on theme/palette changes (no refresh required).
  - ChartOptions derive text/grid colors from CSS variables at draw time.
  - A theme signal triggers redraw when mode/palette updates.
- World selector menu width/structure aligned with page panels so it visually matches content width.
- Add-to-List shows pointer on hover; Toggle thumb visually centered.

6) History & Footer
- History:
  - Tokenized panels/cards; “Clear History” is btn-secondary in light mode.
- Footer:
  - Uses token elevated background (white in light mode), outline token for border.
  - Patreon shows pointer; floating easter egg overlay uses default cursor.

7) Analyzer
- Sort by Profit/ROI replaces existing value (no duplicate query keys).
- Chips (HQ/ROI/etc.) now follow token readability standards in light mode.
- ROI emphasis tuned so high-ROI looks slightly more emphasized than low.

---

## Previous Foundation (Still in Effect)

- Theme architecture (tokens for text/outline/brand; light-mode overrides; palette system via data-palette; SSR first-paint guard).
- Utilities adopted (panel, card, btn-*, nav-link, input/select/textarea/kbd).
- Global shell and multiple pages previously swept for tokenization and contrast.

---

## Known Issues / Deferred

- Job Sets in Item Explorer: still not showing as expected. This appears tied to game/data shape changes. We added resilience (fallback to job.name, case-insensitive matching by abbreviation or English name, URL decoding), but we will revisit functionality after data stabilizes.
- Some fine-grained contrast cases may still appear on certain displays/palettes; ongoing micro-adjustments.

---

## Next Steps (Execution Order)

1) Currency Exchange (finish UX polish)
- Add “Clear All” button for active chips.
- Align sort header visuals with chip/filter state for maximum clarity.
- Optional: Preset filter bundles (power users).

2) Item Explorer (layout + performance)
- Mobile-first improvements:
  - Switch smallest breakpoints to a compact two-column layout to prevent awkward wrapping.
  - Keep price and add-to-list inline on small screens.
- Desktop sidebar animation:
  - Reduce animation cost (duration, transform intensity).
  - If perf issues persist on desktop, disable the animation at lg+ breakpoints.

3) Search
- Re-scan for any low-contrast remnants in the dropdown and ensure consistent token usage.
- Consider lightweight input throttling (if needed on mobile) now that fuzzy is optimized.

4) Nav (Right cluster on mobile)
- Validate multi-row wrap behavior across device sizes and ensure balanced spacing.

5) A11y & QA
- Contrast checks (light/dark) across chips, badges, and small captions.
- Focus rings present on all interactive controls (filters/links/buttons).
- Confirm chart label legibility in both themes across palettes.

6) Optional: Faction-Themed Palettes (post polish)
- Add curated palettes inspired by FFXIV factions:
  - Maelstrom (crimson/brass)
  - Twin Adder (woodland/amber)
  - Ishgard (frost/silver)
  - Crystarium (aurora/pale gold)
  - Old Sharlayan (deep blue/parchment)
  - Tuliyollal (terracotta/jade)
- Wire to palette toggles and token system with both light/dark tuning.

---

## Acceptance Checklist (Spot-Check)

- Currency Exchange:
  - Quick filters labeled and usable.
  - Inline min/max controls working.
  - Active chips with per-chip clear; (next) clear-all present.
  - Main amount label feels intuitive for users.

- Item Explorer:
  - Desktop grid rows: Add-to-List never wraps; price aligned; spacing consistent.
  - Light-mode labels readable and clearly “active” vs “inactive.”
  - Sidebar open/close feels performant.

- Nav (Right):
  - All items (Login/Settings/Logout/Invite) are visually consistent and look good at small widths.

- Search:
  - Results readable in light/dark; hover/active subtle but clear.
  - Feels responsive on mobile.

- Item View:
  - Chart text/grid updates immediately with theme changes; legible in both modes.

---

## Build & Commit

- Build:
  - Use “cargo leptos build” for a full pass (ensures styles are up to date).
  - Use “cargo build” to validate server-side code paths where needed.

- Commit discipline:
  - Group changes by feature or route.
  - Avoid committing broken states; aggregate cohesive UX pieces per commit.

---

## Appendix: Utilities Reference

- Panels/Cards: `panel`, `card`
- Text: `--color-text`, `--color-text-muted`, `--brand-fg`
- Borders/Dividers: `--color-outline`
- Tinting: `color-mix(var(--brand-ring) X%, transparent)`
- Buttons: `btn-primary`, `btn-secondary`, `btn-danger`
- Links-as-tabs: `nav-link`
- Inputs: `input`

---

## Where To Resume

- Currency Exchange:
  - Add “clear all” to filter chips bar.
  - Verify sort header affordances.

- Item Explorer:
  - Mobile layout pass (2-column), ensure inline CTA aligns with price.
  - Reduce sidebar animation cost on desktop.

- Search/Nav:
  - Final contrast/perf micro-sweeps.
  - Confirm nav right cluster wrapping behavior on small screens.

- After UI polish: implement faction-themed palettes.