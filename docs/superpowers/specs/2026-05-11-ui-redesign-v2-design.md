# Ultros UI Redesign v2 — Design

**Date:** 2026-05-11
**Status:** Awaiting user review
**Scope:** Site-wide layout, navigation, and density overhaul. Replaces the top-nav-only shell with a sidebar + slim topbar; demotes the card-heavy visual vocabulary in favor of compact toolbar filters and flat surfaces; rewrites the home page around live market data; restructures the item view to look like a marketboard tool, not a card grid. Existing theme/palette system (14+ palettes, dark/light) is preserved unchanged.

---

## Why

The current frontend ([lib.rs](../../ultros-frontend/ultros-app/src/lib.rs)) has accumulated three structural problems that the screenshots in this conversation make obvious:

- **Vertical-only nav doesn't scale to the tool count.** The `AppsMenu` dropdown ([apps_menu.rs](../../ultros-frontend/ultros-app/src/components/apps_menu.rs)) hides ten+ analyzer tools behind one click. Users can't see what Ultros *is* without opening a menu, and there's no muscle-memory home for cross-tool navigation.
- **`max-w-7xl` plus heavy cards waste horizontal real estate.** [tailwind.css:1419-1487](../../style/tailwind.css:1419) caps content at 80rem and centers it, then `.card` / `.feature-card` add 1rem+ of padding, gradient backgrounds, hover scale animations, and shadow rings on top of every panel. The result on a 1440p display is a narrow column of decorated cards surrounded by void.
- **Home page is a card grid, not a tool.** [home_page.rs](../../ultros-frontend/ultros-app/src/routes/home_page.rs) uses `feature-grid` with square `feature-card`s for every analyzer link. The most distinctive thing Ultros has — live market data flowing in real time — is buried.

The reference screenshots the user shared (a Flip Finder with a compact filter toolbar, a left sidebar listing all tools, and a wide results table; and an Item view with flat stat sections, full-width sale chart, and inline listings table) show what density and navigation should look like.

## Non-goals

- Re-theming. The palette system, `data-theme` light/dark, and all 14 palettes stay as they are. This is a layout + chrome + density change.
- API or data-model changes. No new endpoints, no schema migrations, no new server-side state.
- Killing `.card` / `.feature-card` from the CSS. The utilities stay defined so any caller not yet migrated keeps rendering; we just stop using them on the redesigned routes.
- New analyzer features. This spec restyles existing tools; it doesn't add Confidence columns, onboarding wizards, etc. (those live in their own specs).
- Mobile-specific feature parity beyond what already exists. Mobile gets a working drawer + readable layouts; we don't redesign mobile-only flows.
- i18n string changes. Sidebar / topbar / home use the existing keys where they exist; new strings get added as English-first with `t!` macros so they're translatable later.

## Architecture

Three new shell components plus one new filter primitive, then per-route migration.

```
AppShell
├── SideNav         (persistent on desktop, drawer on mobile)
├── TopBar          (slim; search + lang/theme/user)
└── RouteOutlet
    └── (route content — no max-w cap, no card chrome)

Toolbar              (replaces FilterCard usages on analyzer routes)
```

### Files added

- `ultros-frontend/ultros-app/src/components/app_shell.rs` — the layout grid
- `ultros-frontend/ultros-app/src/components/side_nav.rs` — primary nav, sections, collapse state
- `ultros-frontend/ultros-app/src/components/top_bar.rs` — slim topbar (search + actions)
- `ultros-frontend/ultros-app/src/components/toolbar.rs` — horizontal filter bar primitive

### Files modified

- `ultros-frontend/ultros-app/src/lib.rs` — `AppInner` adopts `AppShell`; current `NavRow` is removed once `TopBar` ships.
- `style/tailwind.css` — new `@utility` blocks for `app-shell`, `side-nav`, `toolbar`; tightened spacing tokens; replaces `app-route-shell`'s `max-w-7xl` cap with sidebar-aware grid.
- Route files migrated one at a time (see Phasing). The first migration is `home_page.rs`; the second is `analyzer.rs` (Flip Finder).

### Files deprecated (not deleted)

- `ultros-frontend/ultros-app/src/components/apps_menu.rs` — folded into `SideNav`; remove only after all routes are off the topbar version.
- `ultros-frontend/ultros-app/src/components/filter_card.rs` — kept until all analyzer routes migrate to `Toolbar`.

## Components

### 1. AppShell

A CSS grid that owns three areas: sidebar, topbar, content. Replaces both `NavRow` and `app-route-shell`.

Grid template:

| Breakpoint | Columns | Rows |
|---|---|---|
| `< 1024px` (mobile/tablet) | `1fr` (sidebar is overlay drawer, not in grid) | `56px topbar / 1fr content` |
| `≥ 1024px` (desktop) | `240px sidebar / 1fr content` *(or `56px sidebar / 1fr` collapsed)* | `56px topbar / 1fr content` |
| `≥ 1536px` (desktop + ad rail) | `240px sidebar / 1fr content / 240px ad rail` | same |
| `≥ 1660px` (wide) | `240px sidebar / 1fr content / 300px ad rail` | same |

The current 1536px / 1660px ad-rail breakpoints are kept; the sidebar takes 240px off the left of what was previously a centered 80rem block, and the content area becomes fluid in between.

Content padding inside the grid cell: `1rem` mobile, `1.5rem` desktop. **No `max-width` cap** on the content cell — tables and lists may run edge-to-edge of the available width. (Reading-width-sensitive routes — `/about`, legal pages — can opt into a `max-w-prose` wrapper themselves.)

Sidebar collapse state lives in a context provider (`SideNavCollapsed: RwSignal<bool>`) persisted to `localStorage` via the existing cookies/storage pattern used by [theme.rs](../../ultros-frontend/ultros-app/src/global_state/theme.rs). Default: expanded on desktop, collapsed (drawer-closed) on mobile.

### 2. SideNav

Persistent left rail on desktop. Sections:

- **Brand:** Ultros logomark + wordmark. Click → home. In collapsed state, logomark only.
- **Home** — single top-level link.
- **TOOLS** — section header (hidden in collapsed state), then: Flip Finder, Vendor Resale, Recipe Analyzer, FC Crafting, Leve Analyzer, Market Trends, Scrip Sources, Venture Analyzer, Item Explorer, Currency Exchange. (These are the existing routes from [lib.rs:311-373](../../ultros-frontend/ultros-app/src/lib.rs:311).)
- **SAVED** — Lists, Retainers, Alerts, Recently Viewed. Only shown for authenticated users.
- **QUICK PRESETS** — page-contextual. On `/flip-finder`, shows preset chips (Fast Flips / High ROI / Big Profit / Low Risk / Vendor Arbitrage). Otherwise hidden. Implemented as a `slot` that the route fills via context.
- **Footer:** version/git-hash link, Discord/GitHub icons (the existing footer's links collapse to this rail; the page-bottom footer goes away on routes with the sidebar).

Each item is a Leptos `<A>` with `aria-current="page"` styling that follows the existing `.nav-link` pattern but in a vertical orientation.

Collapse behavior:
- Toggle button at top of sidebar (chevron). Desktop only.
- In collapsed state: 56px wide, icon-only items, tooltips on hover showing the label (using existing `Tooltip` component). Section headers hidden. Quick Presets hidden (too dense for icons).
- Mobile: no in-rail toggle; the topbar hamburger opens the sidebar as a full-height overlay drawer with a backdrop. Drawer dismisses on route change, backdrop click, or escape.

### 3. TopBar

Single horizontal row, 56px tall, full width of the topbar grid cell.

- **Left:** on mobile, hamburger button toggling the sidebar drawer. On desktop, a breadcrumb showing the current section name (derived from route — e.g. "Flip Finder", "Item Explorer", "Gilgamesh"). No "Home" link here (sidebar has it).
- **Center:** `<SearchBox>` ([search_box.rs](../../ultros-frontend/ultros-app/src/components/search_box.rs)), expanded to fill available space, capped at ~480px so it doesn't dominate ultra-wide layouts. Keyboard shortcut `⌘K` / `Ctrl+K` to focus.
- **Right:** `<LanguagePicker>`, theme/palette toggle (existing `<QuickThemeToggle>`), `<UserMenu>`. Order matches the reference screenshot.

The current `NavRow`'s mobile fallback ("search row on top, actions row below") is no longer needed because the sidebar absorbs Home and AppsMenu. The topbar fits in one row even on mobile: the breadcrumb is hidden, and lang/theme/user collapse into a single overflow menu under a vertical-dots button (see Open Question #2 for the alternative — keeping them always-visible).

### 4. Toolbar

A flat horizontal filter row. Replaces the `.filter-card` rounded panel currently wrapping each analyzer's filters.

Anatomy (left to right):
- Section title (page title — e.g. "Flip Finder for Gilgamesh"), optional world selector immediately under it, on its own line above the toolbar
- Toolbar row: labeled inputs in flex layout (Profit Min, ROI Min, Sales Min, Buy Price Max), then pill groups for booleans (Pre-tax/Post-tax, Local/Cross-region), then a **More Filters** button at the far right that opens an inline expandable row of secondary filters
- Active filters render as dismissible chips under the toolbar on small viewports where they wrap

Implementation: a single `Toolbar` parent that takes `children` slots. Each labeled input is a small reusable `ToolbarField` with `label` and `child` props. Pill groups use a `ToolbarPills` component wrapping the existing `<Toggle>` / segmented-control pattern. No bespoke per-analyzer styling — analyzer routes compose `ToolbarField`s from their existing filter signals.

The compactness comes from:
- Field heights of 36px (vs current ~44px in `.input`)
- Labels above fields at `text-xs uppercase text-muted` rather than inline placeholder hints
- Single 1px outline, no gradient, no shadow, no card border

### 5. Home page

Drops `feature-grid` entirely. New structure top-to-bottom:

1. **Hero band** — full-width, no card chrome. Bold, modern. Left side: large Ultros wordmark in a custom display weight (replacing the Pacifico script — see "Open Questions"), tagline, two CTAs (primary: "Open Flip Finder", secondary: "Browse Items"). Right side: an *animated, server-streamed* visual element — see "Hero visual" below.
2. **Live Market Pulse** — a tall horizontal strip showing real-time sale activity. Promotes [live_sale_ticker.rs](../../ultros-frontend/ultros-app/src/components/live_sale_ticker.rs) to a first-class hero feature: gil amounts animate in, item icons fly through, datacenter labels swap. No card wrapper.
3. **Today's Top Deals** — a compact 5-row dense table pulled from the Flip Finder API for the user's home world. Each row clickable into the item view. Pre-filtered with the "Fast Flips" preset so the home page shows actually-actionable rows, not the analyzer's default chaos.
4. **Tools rail** — a single horizontal scrollable row of icon+label chips for all tools. Replaces the 8-card grid. Each chip is `~96px wide × 80px tall`, no aspect-square forcing, no hover-scale, single-line label below icon. Mobile: still horizontal, scrolls.
5. **Inline ad slot** (collapsible) — under the tools rail.
6. **Recently Viewed** ([recently_viewed.rs](../../ultros-frontend/ultros-app/src/components/recently_viewed.rs)) — if signed in or has cookie history, a horizontal item-icon strip.
7. **Footer** — same footer as today but lighter chrome.

#### Hero visual (push the stack)

The user asked for bold/creative pushing the tech stack. Concept: a **live-streaming gil-flow visualization** in the hero. Implementation sketch:

- Leptos SSR sends the initial frame as static SVG (so the hero is visible pre-hydration, no FOUC)
- On hydrate, a `<canvas>` (or SVG with `<animate>` elements) takes over
- A WebSocket subscription to the existing live-sales feed ([ws/](../../ultros-frontend/ultros-app/src/ws/)) drives particles: every confirmed sale spawns a glowing gil coin that traces a path from one DC label to another, fading as it goes
- Particle density tracks actual sale volume — slow market = sparse, busy market = dense
- Color matches the active palette via CSS custom properties read in JS
- Reduced-motion preference disables the animation; static SVG stays.

This is the kind of visual that says "this is a marketboard tool" rather than "this is a generic SaaS landing page" — and it shows off Leptos's SSR-then-hydrate-then-WS story without any framework branding noise.

If the WebSocket visual proves complex enough to slip the phase, the fallback is a static hero with a large sparkline of one popular item's price history (still server-rendered, still on-brand) plus the live ticker strip below. The fallback is good enough to ship.

### 6. Item view

Restructured to match reference screenshot 2. From the top:

1. **Item header band** — full-width, flat. Item icon at 80px square, item name in a large weight (no gradient), item level pill, item type subtitle, description paragraph, copy-to-clipboard button. On the right: a **Market Price** stat group — current world's lowest price as the hero number, 7d delta as a small green/red percentage chip, last-updated timestamp. No card border around any of this — just spacing.
2. **External-links row** — Universalis, Garlandtools, Teamcraft icons (existing affordances). Inline under the header band.
3. **Region/DC/World selector** — uses the new `Toolbar` pattern. Region tabs, then DC tabs, then world tabs below.
4. **Stats + chart row** — split 1:2 on desktop. Left: a 2×2 grid of stat blocks (Current Lowest, HQ Current Lowest, Recent Average, Active Listings) with thin dividers, no card chrome. Right: the [price_history_chart.rs](../../ultros-frontend/ultros-app/src/components/price_history_chart.rs) at full height with timeframe pills (7d/30d/90d/All) above it. On mobile: stacks vertically, chart goes first.
5. **Listings table** — full-width, sticky header, dense rows. The existing [listings_table.rs](../../ultros-frontend/ultros-app/src/components/listings_table.rs) — restyled to remove its card wrapper and use the same row density as the analyzer table.
6. **Sale history table** — same treatment, under listings.
7. **Crafting recipe info** (if applicable) — inline section, single row showing "Craft for ~X gil" with a link.

### 7. Item explorer sidebar

The current `/items` route renders its own categories sidebar inside the route. Restyle it as follows:

- Width drops to 200px (from current ~256px)
- Removes the card surface — flat list of categories with hover backgrounds, no rounded panel
- Adds a "filter categories" text input at the top (filters the visible category list, doesn't navigate)
- Selected category gets the existing brand-tinted background style from `.nav-link[aria-current="page"]`
- On mobile, the categories rail collapses to a `<details>` disclosure at the top of the content, not a separate sidebar

This makes the item explorer's category list read as a *route-local secondary nav* rather than a competing chrome against the global SideNav.

### 8. Visual system tightening

- `style/tailwind.css` — add new utilities `@utility app-shell`, `@utility side-nav`, `@utility side-nav-item`, `@utility top-bar`, `@utility toolbar`, `@utility toolbar-field`. These are flat-by-default (1px border, no gradient, no shadow on hover). Existing `.card`, `.feature-card`, `.panel` utilities stay defined for back-compat.
- Replace `.app-route-shell` (the centered-80rem container) with a `.app-shell` grid as described in §1. The current `.app-ad-rail` block continues to occupy the right grid column at ≥1536px.
- Default route content padding drops from `p-4`/`p-6` to `p-3`/`p-4`.
- Tables drop the existing `padding: var(--spacing-md)` per-cell (10px) to `padding: 0.5rem 0.75rem` (matches the reference screenshot density). Affects [tailwind.css:602-619](../../style/tailwind.css:602).
- Buttons (`.btn-primary`, `.btn-secondary`, `.btn-ghost`) keep their current styling — they read as intentional accents against the new flat surfaces, not as competing chrome.

## Data flow

No new data sources. The redesign reuses:

- Existing routes module ([routes/](../../ultros-frontend/ultros-app/src/routes/))
- Existing API client ([api.rs](../../ultros-frontend/ultros-app/src/api.rs))
- Existing live-sales WebSocket ([ws/](../../ultros-frontend/ultros-app/src/ws/))
- Existing theme/palette/cookies contexts

The one new piece of client state is the sidebar collapse signal, persisted via the same `Cookies` pattern as theme. It's read in `AppShell` and consumed by `SideNav`; no other component cares about it.

The page-contextual "Quick Presets" section in the sidebar is implemented as a `RwSignal<Option<View>>` context provided by `AppShell` and filled by analyzer routes on mount. When the route is left, the signal is cleared.

## Error handling

The redesign is structural, not stateful — there are no new failure modes. Existing error boundaries on routes are unchanged. The live-streaming hero visual on the home page must:

- Render a static SSR fallback that's complete and visually correct even if hydrate or the WebSocket never runs
- Fall back gracefully if the WebSocket connection fails (the visual freezes on the last frame; existing reconnect logic in the WS layer handles recovery)
- Respect `prefers-reduced-motion` — no particles, static SSR frame stays

## Testing

Integration smoke (`./scripts/run_e2e.sh`) runs the Puppeteer harness in [integration/](../../integration/) against a curated route list at desktop and mobile breakpoints. Per phase, we update:

- The route list if any new routes land (none planned)
- The mobile breakpoint expectation — drawer-closed sidebar, hamburger visible, search reachable
- The desktop breakpoint expectation — sidebar present and active item highlighted matches the current route

Visual regressions are caught by the existing screenshot diff in the harness; that becomes the de-facto acceptance test for "the new shell still works on every page."

No new unit tests; this is structural code without business logic to test in isolation.

## Phasing — parallel tracks

The user asked for parallel tracks, not strict ordering. Track 1 unblocks tracks 2–5 because it lands the shell; once it merges, the rest can proceed in parallel and ship in any order.

```
Track 1 (foundation; blocks the rest):
  Shell + SideNav + TopBar + Toolbar primitive
  Migrate `lib.rs` to AppShell; keep all routes rendering their current contents inside the new shell
  Ship: full app works in the new shell, with old card-heavy route bodies still intact

Tracks 2, 3, 4, 5 (parallel after Track 1 merges):
  Track 2 — Home page redesign (incl. hero visual; fallback static hero ships first if needed)
  Track 3 — Flip Finder + Vendor Resale toolbar migration
            (Recipe Analyzer, FC Crafting, Leve Analyzer, Trends, Venture Analyzer, Scrip Sources follow in the same track)
  Track 4 — Item view redesign
  Track 5 — Item explorer sidebar restyle

Track 6 (after all of 2–5 merge):
  Cleanup — audit unused .card / .feature-card / FilterCard / AppsMenu callers; delete the ones with zero references
```

Each track is its own implementation plan, gets its own PR, and can be reviewed/merged independently. The app is shippable between every PR — no half-redesigned states reach `main`.

## Open questions

1. **Pacifico hero font.** I'm proposing to retire the `Pacifico-Regular` script for the hero in favor of a bolder modern sans (or a custom-weight Jaldi treatment) to match the "bold, creative" direction. Final font choice is a Track 2 design call, not a Track 1 blocker. If you want to keep Pacifico, say so and I'll preserve it.
2. **Mobile overflow menu.** On viewports too narrow for `lang + theme + user` to fit alongside search, the right-side topbar controls collapse into a single vertical-dots button that opens a small menu. Acceptable, or should they stay always-visible and let mobile users scroll the topbar horizontally?
3. **Sidebar persistent footer.** I'm putting version/git-hash and Discord/GitHub links at the bottom of the sidebar so the page-bottom `<Footer>` can go away on routes with the sidebar. Routes that *should* keep the big page-bottom footer (about, legal): they'll opt back in via a `show_footer` prop on AppShell, or always render their own footer inside the route body. Approve the "sidebar replaces footer on tool routes" direction?

Resolutions go into the implementation plans, not back into this spec.

## Risks

- **Hydration ordering of the hero visual.** Leptos SSR → hydrate → WebSocket-driven canvas is non-trivial. Mitigation: SSR a complete static frame; only attach the canvas listeners post-hydrate; design the canvas to be visually compatible with the static frame so the swap is invisible.
- **i18n string drift.** New strings (sidebar section headers, "More Filters", tool descriptions on the home rail) are added in English first; localized strings land separately. Risk is low because the existing `t!` macro infrastructure already supports adding keys.
- **Density on mobile.** Tighter padding helps desktop but can hurt thumb-tappability on mobile. The 56px topbar and 44px sidebar drawer items meet the iOS/Android 44px tap-target guideline. Tables stay at their current row height on mobile (≥44px) — only desktop rows get denser.
- **Ad-rail interaction with the sidebar.** At 1536px, sidebar (240px) + content + ad rail (240px) = 1056px of fixed columns, leaving 480px of fluid content. That's still readable for analyzer tables (Flip Finder reference fits comfortably). At 1660px it widens further. No regression vs today's behavior at those breakpoints.
