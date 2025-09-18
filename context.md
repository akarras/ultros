# Homepage & Footer Polish — Current Context

This document replaces prior context. It summarizes the latest UI polish to the homepage and footer, the design primitives used, and what to verify next.

---

## tl;dr

- New hero section on the homepage with a gradient title, concise tagline, and three primary calls-to-action.
- Feature cards upgraded to use the new feature-card title/description utilities with subtle motion and sheen.
- Footer links now use consistent button-like visuals with icons, plus a divider separating links from metadata.
- Build verified end-to-end using the full pipeline (including Tailwind) to ensure styles are up to date.

---

## What Changed

1) Homepage hero
- Added a prominent “Ultros” gradient title and a short, friendly subtitle.
- Introduced clear CTAs:
  - “get started” (book)
  - “invite bot” (Discord, with icon)
  - “open flip finder”
- Included a branded visual tile on the right for large screens (rounded, elevated, subtle blur).
- Wrapped in a tokenized “panel” with tasteful background treatment that adapts to theme/palette.

2) Feature cards
- Converted headings to use `feature-card-title` (animated underline accent on hover).
- Converted copy to use `feature-card-desc` (multi-line clamp, better contrast).
- Retained icons and badges; kept grid layout responsive and consistent with the design system.

3) Footer
- Replaced raw text links with `btn-ghost`-style links and appropriate icons where relevant:
  - Discord
  - GitHub
  - Book
- Inserted a `divider` between the link row and the text metadata block.
- Preserved existing version hash, copyright, and third-party attribution.

---

## Touched Code (for orientation)

- Homepage: `routes/home_page.rs`
  - Introduced new hero layout, updated feature card content classes.

- Footer: `lib.rs` (`Footer()` component)
  - Swapped raw links to `btn-ghost` style, added icons for GitHub and Book, and added a `divider`.

No logic or data flows were changed—purely presentational improvements.

---

## Design System Primitives Used

- Containers and surfaces:
  - `panel`, `card`, `elevated`, `surface-blur`, `divider`
- Buttons and links:
  - `btn-primary`, `btn-secondary`, `btn-ghost`, `nav-link`
- Feature cards:
  - `feature-card`, `feature-card-icon`, `feature-card-title`, `feature-card-desc`, `feature-badge`, `feature-grid`
- Inputs and misc:
  - `input`, `muted`
- Tokens:
  - Colors and theme/palette variables (light/dark + runtime palette swaps)
  - Outline, brand ring, brand fg/bg
  - Decorative gradients via `--decor-spot`

All styles are backed by `style/tailwind.css`, which centralizes tokens and utilities for consistent behavior in light/dark modes.

---

## Accessibility & Interaction

- Focus visibility preserved via tokenized rings and outlines.
- Links-as-buttons maintain keyboard accessibility and clear focus states.
- Feature-card interactions rely on subtle transform/opacity changes to avoid motion sickness while remaining discoverable.

---

## Build & Verification

- Full style and app build path validated using the standard build process (ensures Tailwind/Tokens/Assets are coherent).
- No new routes or SSR behaviors were introduced; hydration and meta contexts remain as before.

---

## QA Checklist

- Hero
  - Title and subtitle readable in both light/dark themes and across palettes.
  - CTAs render correctly across responsive breakpoints and are clearly distinct.
- Feature cards
  - Hover sheen/underline effects only trigger on pointer devices; baseline remains clean on touch devices.
  - Text clamping looks correct for longer strings.
- Footer
  - Buttons render with the correct icons and spacing.
  - Divider displays with appropriate contrast in light/dark.
- General
  - No layout shift on initial paint (theme guard intact).
  - Nav and hero do not overlap; sticky nav remains legible over content.
  - No hydration warnings/regressions.

---

## Next Steps

- Homepage:
  - Optional: add a lightweight metrics row (e.g., “listings indexed”, “active users,” etc.) under the hero panel if desired.
  - Optional: swap the hero tile icon for a small animated preview or sparkline that respects theme tokens.

- Footer:
  - Optional: localize link labels in the future if i18n is introduced.

- Global UX:
  - Re-sweep small-screen spacing in the hero/CTA cluster to ensure tight visual rhythm on mobile.
  - Continue palette QA for extreme contrast displays.
