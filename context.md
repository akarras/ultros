# Theming Status — Dark/Light, Palettes, and UI Unification (In-Progress)

This document captures the current state of the theming/migration, what’s already shipped, and the next steps to finish unifying the UI across the app. Use this to resume work tomorrow.

---

## What’s Implemented

1) Theme architecture
- CSS tokens for neutrals, outlines, text, and brand semantics added to Tailwind build.
- Light-mode overrides via [data-theme="light"] including:
  - Flipped brand semantics suitable for light backgrounds
  - Softer “decor spot” background highlights
- Runtime palette system via [data-palette="<name>"] with: violet, teal, emerald, amber, rose, sky.
- SSR first-paint script sets data-theme and data-palette prior to hydration to avoid FOUC.
- Global ThemeSettings (mode + palette) with persistence (localStorage + cookie). Providers wired at app root.

2) Core utility tokens adopted
- panel, card, btn/btn-primary/btn-secondary/btn-danger, nav-link, input/select/textarea/kbd
- Links: higher contrast in light; hover background uses brand-ring mix; nav-link ignores generic anchor styles.
- Background/gradients: replaced black/white literals with token-based color-mix.

3) Top-level shell
- Background uses var(--color-background) and var(--decor-spot) radial highlight (no raw black/white).
- Quick theme toggle uses nav-link styling for better legibility in light.

4) Home page
- “Welcome to Ultros” and body copy use readable token colors (no gradient text for critical headings).
- Feature cards: softened gradients, tokenized tints; higher-contrast icons/titles.
- Recently Viewed + Recent Sales:
  - Tokenized panel and card rows (no bg-black/30 or border-white/5).
  - Headings/text/metadata moved to var(--color-text)/var(--color-text-muted).
  - Scrollbar and hover states toned to tokens.

5) Search
- Search input uses input utility and tokens (background, outline, placeholder).
- Search results container toned down (removed old neon pink accents).

6) Analyzer
- Filter cards use panel; titles use var(--brand-fg), descriptions use var(--color-text-muted).
- Numeric inputs use input utility; placeholder/focus ring tokenize to brand.
- Preset filter buttons use btn-secondary.
- Filter chips unified to token badge pattern:
  - bg: color-mix(var(--brand-ring) 14%, transparent)
  - text: var(--brand-fg)
  - border: var(--color-outline)
- Results summary uses panel.
- Table:
  - Container panel; header row uses subtle brand-ring tint for clarity in light.
  - Zebra/hover rows via color-mix (token-driven).
  - Active-sort header (profit/ROI) uses var(--brand-fg) for high contrast.
- Toggle component redesigned:
  - Token-based track/outline; checked state uses brand-ring tint.
  - Larger thumb, improved focus ring; label uses text-muted → text on hover.
  - Fixes “cross region enabled”/“japan enabled” readability in light.

7) Lists
- Create/Edit list UI is a panel with input utility; actions are btn-primary/secondary/danger.
- Lists table wrapped in panel; tokenized headings/links.

8) Retainers
- Header actions reworked as horizontal “tabs” with nav-link (Edit / All Listings / Undercuts); icons normalized (~1.25em).
- Listing tables and undercut tables converted to panel; table styling remains as-is but inherits token colors.

9) World selector (Select + WorldPicker)
- Select input now uses input utility; dropdown is a panel (no brand-black gradient).
- Menu rows use brand-ring hover mix; captions use text-muted.
- Fixes “Select World” box readability in light mode.

10) Item view
- Removed purple/black gradients and white text blocks; replaced with panel/card tokens.
- Sticky world menu uses panel.
- Chart wrappers enforce text color via tokens; chart axis/grid already read CSS variables.
- Related items container converted to panel; more local sweeps pending (see next steps).

11) Item explorer (first pass cleanup)
- Sidebar uses panel; removed purple→black gradients.
- “Browse/Close Categories” button uses btn-secondary (no garish gradient).
- Mobile overlay uses token-based dim color instead of black.

---

## Pages/Components To Sweep Next

High priority
- Sale history table (components/sale_history_table.rs):
  - Tokenize chips/badges and row borders, remove any text-gray/* or border-white/*.
- Related items (components/related_items.rs):
  - Convert row cards to card utility; remove bg-black/30 and border-white/* leftovers.
- Profile/header actions:
  - Ensure tokenized buttons/links for consistency (profile_display.rs).
- Lists view page tables (routes/list_view.rs if present):
  - Ensure tables and actions use panel/buttons tokens.

Medium priority
- Analyzer: further soften link hover tints if still too saturated in light; ensure all “text-brand-300” remnants are swapped to token link style where needed.
- Item explorer: deeper layout improvements (category density, clarity of calls to action).
- Any legacy scrollbar-white/track-transparent styles → token scrollbars where appropriate.

Low priority
- Seasonal/alt palettes; optional custom hue generator.
- PWA-like theme-color meta managed per theme (a basic heuristic is already in place).

---

## A11y/QA

- Contrast (light/dark):
  - Headings, active states, and badges meet AA where practical.
  - Analyzer header active-sort (profit/ROI) verified; chips and badges use sufficient contrast in light and dark.
- Focus rings:
  - Toggles and nav links have clear focus rings; expand to any interactive tokens still missing ring treatment.
- Charts:
  - Client charts use CSS vars; item chart wrappers enforce readable text color.

---

## Known Issues (to revisit)

- Nav bar “Light” label and certain nav items may still feel light in some palettes; adjust nav-link border/background mix one notch if needed.
- Analyzer hyperlink intensity in light may still be slightly pink on some displays; can reduce hover mix or boost plain link foreground to brand-fg proportionally.
- Some tables still use legacy sticky header/row colors — move to token backgrounds as we touch each route.
- Server-rendered item cards (SVG) don’t inherit CSS; we added optional chart options previously, but further palette parity may require carefully chosen static colors or injecting theme hints.

---

## Files Touched (recent batch)

Core styles/tokens
- style/tailwind.css
- style/legacy.css

Global/App
- ultros-frontend/ultros-app/src/lib.rs
- ultros-frontend/ultros-app/src/global_state/mod.rs
- ultros-frontend/ultros-app/src/global_state/theme.rs
- ultros-frontend/ultros-app/src/components/theme_picker.rs
- ultros-frontend/ultros-app/src/components/toggle.rs

Home
- ultros-frontend/ultros-app/src/routes/home_page.rs
- ultros-frontend/ultros-app/src/components/recently_viewed.rs
- ultros-frontend/ultros-app/src/components/live_sale_ticker.rs

Search
- ultros-frontend/ultros-app/src/components/search_box.rs

Analyzer
- ultros-frontend/ultros-app/src/routes/analyzer.rs
- ultros-frontend/ultros-app/src/components/number_input.rs

World selector
- ultros-frontend/ultros-app/src/components/select.rs
- ultros-frontend/ultros-app/src/components/world_picker.rs

Item view
- ultros-frontend/ultros-app/src/routes/item_view.rs
- ultros-frontend/ultros-app/src/components/price_history_chart.rs

Item explorer
- ultros-frontend/ultros-app/src/routes/item_explorer.rs

Lists/Retainers
- ultros-frontend/ultros-app/src/routes/lists.rs
- ultros-frontend/ultros-app/src/routes/retainers.rs

---

## Next Steps (execution order)

1) Tables and chips
- sale_history_table.rs: replace hardcoded text-gray/*, border-white/*; add token chip utility; unify price chip colors to token palette.
- related_items.rs: use card utility for rows; fix bg/border/text via tokens; ensure hover/active matches theme.

2) List/profile cleanup
- list_view.rs (if present) & profile_display.rs: buttons/links → button utilities; tokens for text/borders.

3) Item explorer redesign (phase 2)
- Rework layout density and category grid visuals:
  - Use card utilities for category links.
  - Optional: add quick filters/tags.
  - Reduce large gradients and busy backgrounds; emphasize content.

4) Nav readability (light)
- If still low on your screen, bump nav-link mix/border one step:
  - Base bg: brand-ring 18–20% → 20–24%
  - Border: brand-ring 24% → 32–36%

5) A11y pass
- Validate contrast AA on light/dark with a few palettes (links, badges, small captions).
- Ensure all interactive elements have a visible focus state.

6) Optional enhancements
- Palette fine-tuning for light (slightly desaturate or flip scale endpoints per palette).
- Add “chip” utility for consistent filter/pill styling.

---

## Commit (to run locally)

Note: This document can’t run git for you. Suggested sequence:

- Review changes in your IDE.
- Then run:
  - git add -A
  - git commit -m "theme: unify analyzer/lists/retainers/world selector/toggle; item view & explorer cleanup; improve light-mode contrast across UI"
  - git push

---

## Resume Here (tomorrow)

- Start with: components/sale_history_table.rs and components/related_items.rs to finish token adoption for rows/chips.
- Validate analyzer links and nav-link contrast in light; tune mix percentages if needed.
- Begin item explorer layout (phase 2) to reduce cognitive load and remove any lingering gradient-based decor.
