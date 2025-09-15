# Theming Status — Dark/Light, Palettes, and UI Unification (Updated)

This document is the single source of truth for our ongoing retheme: current status, what changed in the latest batch, and what to do next.

---

## What’s Newly Completed (this batch)

High-priority flows and components updated to tokens (dark/light + brand palette), removing hardcoded grays/whites/black and legacy gradients.

1) Currency Exchange (both routes)
- CurrencySelection (the “all currencies” list)
  - Replaced gradient panels with tokenized `panel`/`card`.
  - Search input uses `input` utility; icon color uses `--color-text-muted`.
  - List items use `card`, hover text uses `--brand-fg`.
  - Empty state uses muted token color.
- ExchangeItem (the sales/profit table for a selected currency)
  - Page title and header now use `--brand-fg` (fixes white title issue).
  - Header container uses `panel`.
  - “Amount to exchange” input uses `input`; label uses `--color-text-muted`.
  - Table header/body adopt token table styles (no `bg-gray-*`, `text-gray-*`, or `divide-gray-*`; now use `--color-outline`, `--color-text`).
  - Sort/active labels use `--brand-fg` and underline for clarity in light mode.

2) Sale history table
- Header uses uppercase tokenized style; body rows now divide with `--color-outline`.
- Insight badges (e.g., “Avg price”, “Median price”) use token chip pattern:
  - bg: color-mix(var(--brand-ring) 14%, transparent)
  - border: `--color-outline`
  - label text: `--color-text-muted`
- Titles use `--brand-fg`. Removes all `white/gray/*`.

3) Lists page (nav + Edit Lists)
- The “Lists” and “Edit Lists” actions are now horizontal “tabs” matching Retainers: `nav-link` with normalized icon sizing.
- Edit Lists header keeps brand title + “Create” action (`btn-primary`), adds nav-link tabs for parity and clarity.

4) Loading skeletons
- Removed dark-only gradients and black panels; now use brand-ring gradient tints and `panel` for row shells.
- Works in both modes without washing out text around them.

5) Item Explorer (first pass of this batch)
- Sidebar category buttons moved from black/gradient to `panel` with token hover/active text colors.
- aria-current now uses `--brand-fg` to indicate active state.
- Additional subtle text color fixes to align with tokens.

6) Related items
- Vendor and related-item rows/cards moved to `card`.
- Ingredient amount badges now use token chip pattern (tint + outline); muted captions tokenized.

7) Modal
- Backdrop uses color-mix with `--color-text` over `--color-background` (no raw black overlay).
- Surface uses `panel`; close button hover/focus uses brand-tinted bg and a proper focus ring.

8) Misc polish
- Search results: item/category text colors tokenized.
- Add-to-list modal rows use `card` and token hover.
- Live sale ticker item text and small item display’s ilvl use token colors.
- Theme picker panel and headings tokenized; copy uses muted token color.
- “Ad” badge uses chip-style token tints instead of black/white overlays.
- Large loading overlay uses brand-ring mixed with background for consistent dim in light/dark.

---

## What’s Already In Place (unchanged from prior context)

- Theme architecture: tokens, light-mode overrides, palette system, SSR first-paint script, persisted settings provider.
- Core utilities: `panel`, `card`, `btn*`, `nav-link`, `input/select/textarea/kbd`.
- Shell, Home, Search, Analyzer (first pass), World Selector, Item View cleanup, and first pass on Item Explorer were previously converted to tokens.

---

## Remaining Work / Next Steps

Targeted sweeps to fully remove lingering grays/whites/black/legacy gradients, and to unify action patterns.

1) List View tables and actions
- Convert remaining “content-well” usage to pure utilities where helpful. The CSS already tokenizes it, but bring structure in line with other rethemed pages:
  - Wrap key content areas in `panel`.
  - Normalize form inputs to `input` and action buttons to `btn-primary` / `btn-secondary` / `btn-danger`.
  - If there are sub-page actions, consider a simple `nav-link` group for clarity, mirroring Retainers/Lists/Explorer patterns.
- Table header/body should rely on tokens: `--color-outline` for dividers, `--color-text` for text, and brand accents for active/sort states if needed.

2) Analyzer refinements
- Reduce any remaining legacy hover color intensity; ensure any residual “text-brand-*” is replaced by token link style or `--brand-fg` where appropriate.

3) Item Explorer (phase 2 layout)
- Improve density/scan-ability of category and item lists:
  - Prefer `card` for grouped elements.
  - Consider subtle section headers and spacing rules using tokens.
  - Keep hover/active to brand-tint and consistent focus rings.

4) Profile display and settings (quick sweep)
- Ensure all header actions/links align with token button/link utilities.
- Remove any `text-gray-*`/`hover:bg-white/*` remnants.

5) Tables everywhere
- If you encounter sticky headers or zebra rows with legacy colors, swap to token backgrounds and outlines, with brand-tinted hovers as needed.
- Keep contrast AA-friendly in both modes.

6) A11y and polish
- Verify small captions, badges, and chips meet contrast in light mode.
- Ensure all interactive elements have visible focus rings (buttons, tabs, toggles, links inside panels/cards).

---

## QA Checklist (light and dark)

- Currency Exchange:
  - Title readable and brand-colored.
  - Search card and list cards legible in light mode.
  - Sales table headers/body have correct contrast; active sort clearly indicated.
- Lists:
  - “Lists” and “Edit Lists” nav links match Retainers tabs visually.
  - Create/Edit list panels and inputs look consistent with tokens.
- Skeletons:
  - Gradients visible but subtle; no harsh black overlays in light mode.
- Item Explorer:
  - No dark gradients in side menu on light mode; hover/active readable.

---

## Build and Validation

- Built the workspace with the Leptos build to surface any Tailwind/tokenization issues:
  - Use: cargo leptos build
- Also validated a regular build:
  - Use: cargo build

Both completed with warnings only (expected in this phase), no errors.

---

## Commit Guidance

After you validate the behavior locally, commit the work:
- Suggested message:
  theme: retheme currency-exchange (lists + sales), sale history, skeletons; unify Lists nav to nav-link; panel/card/token sweep for related items, modal, search results, theme picker, overlays

Push once confirmed working.

---

## Quick Utility Reference

- Panels/cards:
  - panel: section containers and table shells
  - card: small row/clickable containers
- Text:
  - --color-text, --color-text-muted
  - --brand-fg for headings/active states
- Borders/Dividers:
  - --color-outline
- Tints and chips:
  - color-mix(var(--brand-ring) X%, transparent)
- Interactives:
  - Buttons: btn-primary / btn-secondary / btn-danger
  - Links-as-tabs: nav-link
  - Inputs: input (plus size classes as needed)

---

## Resume Here

- List View page: convert inputs/buttons/tables to token utilities and panels; ensure actions mirror Retainers/Lists tabs if applicable.
- Analyzer: soften remaining hover accents; eliminate any straggling non-token text colors.
- Item Explorer: phase 2 layout/card pass to improve scan-ability and consistency.
