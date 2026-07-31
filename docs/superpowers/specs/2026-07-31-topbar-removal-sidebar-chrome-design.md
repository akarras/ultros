# Top bar removal — sidebar-owned chrome

**Date:** 2026-07-31
**Branch:** `claude/top-bar-sidebar-redesign-d938db`
**Status:** design approved, ready for planning

## Summary

Delete the application top bar. The sidebar becomes the sole owner of global
chrome on desktop; below 1024px a fixed bottom bar carries the three controls
that must stay reachable without opening the drawer.

Global search moves out of the top bar into an overlay with three triggers.
The locale switcher and the account/login control move into a single drop-up
anchored to the bottom of the sidebar.

## Motivation

The top bar ([`top_bar.rs`](../../../ultros-frontend/ultros-app/src/components/top_bar.rs))
costs a fixed 56px row on every page to host four controls, three of which are
used rarely (language, theme, account). Folding them into the sidebar returns
that row to content and leaves one navigation surface instead of two.

## Decisions

Each of these was chosen explicitly during design; alternatives considered are
recorded in "Rejected alternatives" below.

| # | Decision |
|---|---|
| 1 | The top bar is removed at every breakpoint, not just desktop. |
| 2 | `SearchBox` moves into an overlay with three triggers: sidebar row, mobile bar, and `Ctrl`/`Cmd`+K. |
| 3 | The sidebar footer holds **one** account row. Locale and theme live inside its drop-up, not as separate rows. |
| 4 | Inside the drop-up, `Language` is an **inline accordion**, not a flyout submenu. |
| 5 | The drop-up opens on **click**, replacing the current hover-driven pattern. |
| 6 | Mobile gets a fixed **bottom** bar with three slots: Menu, Search, Items. |
| 7 | Account is **not** in the mobile bar. Sign-in is drawer-only. |
| 8 | Search and Item Explorer are promoted above the `Tools` header as a pair. |

## Architecture

### Shell

`AppShell` loses its top grid row. `TopBar` is removed from the tree and the
file deleted.

| Viewport | Grid columns | Chrome |
|---|---|---|
| ≥1536px | `240px \| 1fr \| 240–300px` | sidebar + ad rail |
| 1024–1535px | `240px \| 1fr` | sidebar only |
| <1024px | single column | drawer + fixed bottom bar |

The collapsed (`56px`) sidebar rule stays desktop-only, as it is today.

Three shell changes beyond dropping the row:

- `min-height: 100vh` becomes `100dvh`. The current value hides roughly the
  last 90px under mobile Safari's browser chrome.
- `.app-shell-content` gains bottom padding below 1024px equal to the bar
  height, or the bar covers the last row of every table.
- The mobile bar needs `padding-bottom: calc(6px + env(safe-area-inset-bottom))`
  so it clears the iPhone home indicator.

### Sidebar order

```
🔍 Search                    ⌘K
🪼 Item Explorer
──────────────────────────────
🏠 Home
TOOLS      9 analyzers
SAVED      Lists · Groups · Retainers · Alerts
HELP       Discord Bot · Help
──────────────────────────────
👤 Account                    ▲
💬 🐙                    8ea1fa2
```

Search and Explorer are both "find me an item" surfaces — Search jumps to a
known item, Explorer browses when you don't know what you want — so they pair
above the divider. `Tools` becomes an honest list of nine analyzers rather than
nine analyzers plus a database.

The existing utility strip (Discord, GitHub, version hash) is unchanged.

### Account drop-up

One row at the very bottom of the sidebar. The panel is anchored
`bottom: 100%` so it grows **upward** and is never clipped by the viewport
edge.

**Signed in:** Profile · Settings — Language ▾ · Theme — Log out
**Signed out:** Sign in with Discord · Settings — Language ▾ · Theme

The signed-out panel must carry `Language`. Today the logged-out menu is
Login / Settings / Theme with no language entry
([`apps_menu.rs:322-336`](../../../ultros-frontend/ultros-app/src/components/apps_menu.rs))
because locale lived in a separate top-bar control. Omitting it here would
leave non-English visitors with no switcher at all.

`Language` expands in place as an accordion listing all 7 locales. This was
chosen over a right-side flyout specifically because the sidebar *is* the
mobile drawer below 1024px, and a hover flyout has no touch equivalent.

**Collapsed 56px state:** the trigger becomes avatar-only and the panel cannot
remain `left: 6px; right: 6px` — it breaks out to a fixed width (~230px)
overhanging the content area.

**Interaction:** opens on click; closes on Escape, on outside click, and on
navigation. This replaces the `use_element_hover` + `focusin` pattern currently
shared by `UserMenu` and `LanguageNavMenu`
([`apps_menu.rs:199`](../../../ultros-frontend/ultros-app/src/components/apps_menu.rs)),
which has no touch story. Outside-click handling is the one piece of genuinely
new interaction code in this design.

`QuickThemeToggle` moves into this panel and stops being duplicated across two
files.

### Search overlay

A new component wrapping the existing `SearchBox` **unchanged**. Open state is
a global signal so all three triggers share it.

- **Desktop:** centered panel, ~560px wide — wider than the 480px the top bar
  gives it today.
- **Mobile:** full-screen sheet with the input anchored at the **top**.

The top anchoring is load-bearing, not cosmetic. A text input inside a
`position: fixed; bottom: 0` element is the one thing a mobile bottom bar
genuinely cannot do: iOS Safari shrinks the visual viewport but not the layout
viewport when the keyboard opens, so a bottom-anchored input ends up behind the
keyboard or stranded mid-screen, with no pure-CSS fix. Keeping the bar to
buttons and putting the input at the top of a sheet sidesteps this entirely.

Closes on Escape and on selecting a result.

### Mobile bottom bar

Three slots, buttons only, hidden at ≥1024px:

| Slot | Action |
|---|---|
| ☰ Menu | toggles `nav.drawer_open` |
| 🔍 Search | opens the search overlay |
| 🪼 Items | navigates to `/items` |

Menu and Search are structurally required — Menu is the only route to
navigation below 1024px, Search is the overlay's mobile trigger.

## Files

| Action | File |
|---|---|
| delete | `components/top_bar.rs` |
| delete | `components/apps_menu.rs` — `AppsMenu` is dead code (see below) and `UserMenu` is superseded by `account_menu.rs`, so the whole file goes |
| delete | `LanguageNavMenu` in `language_picker.rs` — orphaned by the top bar's removal |
| new | `components/search_overlay.rs` |
| new | `components/mobile_bar.rs` |
| new | `components/account_menu.rs` |
| rewrite | `components/side_nav.rs` — nav order + footer account row |
| rewrite | `components/app_shell.rs` — drop `TopBar`, add `MobileBar` + `SearchOverlay` |
| edit | `components/language_picker.rs` — add the inline accordion variant |
| edit | `global_state/side_nav.rs` — add search-overlay open signal |
| edit | `style/tailwind.css` — remove `.top-bar*`; add `.mobile-bar`, account row, drop-up panel rules; shell grid changes |
| edit | `components/mod.rs` — module list |

### AppsMenu is dead code

[`apps_menu.rs:16-179`](../../../ultros-frontend/ultros-app/src/components/apps_menu.rs)
defines `AppsMenu` with no callers anywhere in the workspace — the only
occurrence of the name outside its own definition is a comment in
`side_nav.rs:21`. It duplicates the sidebar's entire tool list, so it has been
drifting silently. Since this work rewrites the other half of the same file, the
deletion is in scope.

## Internationalization

Per `CLAUDE.md`, every new user-facing string needs a key in **all seven**
locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with a real
translation, not an English stub. At minimum this design introduces:

`search_open`, `account`, `sign_in`, `language`, `theme`, and the three mobile
bar labels (menu / search / items).

Existing keys that survive: `side_nav_tools`, `side_nav_saved`,
`side_nav_aria_primary`, `side_nav_toggle_sidebar`, `login_with_discord`,
`profile`, `settings`, `logout`.

`side_nav_toggle_navigation` currently labels the top bar hamburger; it moves
to the mobile bar button and keeps its key.

## Risks and accepted trade-offs

**Mobile sign-in discoverability drops — accepted.** Today the logged-out
`Login with Discord` button renders on phones: the `hidden md:block` wrappers in
`top_bar.rs` cover only the language picker and theme toggle, so the login
button and `UserMenu` are both visible at every width
([`top_bar.rs:41-63`](../../../ultros-frontend/ultros-app/src/components/top_bar.rs)).
With a three-slot bar and account in the drawer only, a logged-out mobile
visitor has no persistent account affordance and must open ☰ to find one. This
was raised and accepted in favour of a single profile surface.

**Sidebar vertical pressure.** The sidebar carries ~18 links and is
`overflow-y: auto`. Adding the account row and the Search/Explorer pair costs
height the nav list gives up on short viewports. Removing the top bar returns
56px to *content*, not to the sidebar. The nav scrolls, so this is a comfort
issue rather than a correctness one.

**Fixed positioning depends on an existing invariant.** The mobile bar relies on
`body` not being a scroll container. `tailwind.css:108-125` puts
`overflow-x: hidden` on `html` and deliberately not on `body`, with a comment
explaining why. Do not reintroduce `overflow-x` on `body` — it would break the
bar along with every viewport-sticky element.

**Auth state and SSR.** `UserMenu` resolves the user through
`Resource` + `Suspense`. `account_menu.rs` must keep that pattern; the
signed-in and signed-out panels differ in content, so a mismatch between the
server render and the client hydrate would surface as a hydration panic.

## Rejected alternatives

- **Locale as its own sidebar row above account.** The original sketch. Dropped
  in favour of a single row after seeing the footer cost three rows.
- **Right-side flyout for the language submenu.** No touch equivalent, and the
  sidebar doubles as the mobile drawer.
- **Link to `/settings` for language.** Least code, but turns a locale switch
  into a page navigation.
- **Four- or five-slot mobile bar.** Four adds Account; five adds Flip Finder,
  which elevates one tool out of ten and pushes tap targets to ~50px where
  German labels truncate.
- **Keeping a slim mobile-only top bar.** Lowest risk, but leaves two chrome
  layouts to maintain.
- **Inline search input in the sidebar.** Cramped at 240px and dead in the 56px
  collapsed state.

## Verification

`./check_ci.sh` from the repo root before committing.

Clippy should be scoped with `-p ultros-app` — the `ultros` binary's test
target does not link on Windows/MSVC, which is unrelated to this change.

This work is CSS- and layout-heavy, so automated checks only prove it compiles.
Manual verification is required at four widths — ≥1536px (ad rail), 1024–1535px,
just below 1024px, and a 375px phone — plus the collapsed sidebar state, the
signed-in and signed-out drop-up variants, and at least one non-English locale
to confirm the accordion and bar labels fit.
