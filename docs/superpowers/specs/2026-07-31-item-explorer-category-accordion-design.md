# Item explorer: subcategory accordion

**Date:** 2026-07-31
**Status:** Approved, ready for planning
**Touches:** `ultros-frontend/ultros-app/src/components/grouped_nav_popover.rs`,
`ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`,
`ultros-frontend/ultros-app/src/components/mod.rs`, `style/tailwind.css`

## Problem

`/items/*` renders a group pill row (Weapons / Armor / Items / Housing / Job
Sets) over a subcategory selector. The selector today is one of two things,
chosen by `group_uses_popover(group)`:

- a `GroupedNavPopover` — an overlay panel anchored to a trigger button —
  for Weapons (1), Job Sets (5), and any group with more than 8 categories;
- an inline `item-explorer-chip-row` — a horizontally scrolling strip with a
  mask gradient — for the remaining small groups.

The popover trigger reads as rough: it is a dropdown affordance for what is
really the page's primary navigation, it floats over the item list, and it
needs overlay machinery (`absolute`, `z-[100]`, `on_click_outside`, Escape
handling) to behave. Having two different interactions depending on which
pill is selected compounds it.

## Solution

Replace both branches with a single inline accordion. All five groups render
the same control: a header button that expands a panel of subcategory chips
in flow, below the pills and above the item list.

Selecting a new group pill forces the accordion open, so switching tabs
always reveals that tab's subcategories.

## Behaviour

| Trigger | Result |
| --- | --- |
| Click a group pill | Accordion opens (and switches to that group's chips) |
| Click a subcategory chip | Navigates, accordion collapses |
| Click the accordion header | Toggles |
| Load a route naming a category or jobset | Starts collapsed |
| Load bare `/items` | Starts open |
| Browser back/forward | Open iff the destination route names no category |

The collapsed header is labelled with the active subcategory when one belongs
to the selected group, so a collapsed accordion still says where you are. It
falls back to the existing "browse subcategories" prompt otherwise. This is
exactly the label logic the popover trigger uses today.

## Design

### 1. `GroupedNavAccordion`

Rename `components/grouped_nav_popover.rs` to `components/grouped_nav_accordion.rs`
(`GroupedNavPopover` → `GroupedNavAccordion`, `PopoverLink` → `NavLink`,
`PopoverIcon` → `NavIcon`) and update `components/mod.rs`. The item explorer
toolbar is the module's only consumer, so this is a rename in place rather
than a parallel component.

The section data shape is unchanged:

```rust
groups: Signal<Vec<(Option<String>, Vec<NavLink>)>>
```

`Some(header)` renders a role-bucket heading; `None` renders a single
headerless wrap of chips. That is already how the popover distinguishes the
Weapons/Job Sets layout from the flat one.

Prop changes:

- **`open: RwSignal<bool>` becomes a prop**, replacing the component-local
  `RwSignal::new(false)`. The toolbar must force the accordion open when a
  group pill is clicked, so the toolbar owns the signal.
- The trigger becomes an accordion header:
  `<button aria-expanded={open} aria-controls="item-explorer-subcategories">`
  with a chevron rotated 180° when open. The panel carries the matching `id`.
  There is one accordion per page, so a constant id is safe.

Removed: `on_click_outside`, the Escape `on:keydown`, the `NodeRef<Div>`
container, the `use_location`/`pathname` effect (the toolbar owns collapse-on-
navigate now), and the `absolute left-0 top-full z-[100]` panel positioning.

### 2. Expansion mechanism and hydration

The panel expands via `grid-template-rows: 0fr → 1fr` on a wrapper with
`overflow: hidden` on the inner element, not by toggling a `hidden` class.

This matters beyond aesthetics. The existing popover deliberately keeps its
panel in the DOM at all times and toggles only the class attribute, so the
SSR and hydration view trees have identical shape — the discipline that keeps
this route clear of the tachys `failed_to_cast_element` family of panics that
`ItemList` and `collect_job_items_sorted` carry comments about. The grid-rows
technique preserves that property (children are always rendered, only the
track size animates) *and* animates, which a `hidden` toggle cannot.

Because the panel is in flow rather than overlaid, `overflow: hidden` on it
clips nothing that needs to escape.

### 3. Toolbar collapses to one path

In `item_explorer_toolbar.rs`:

- Delete `group_uses_popover()`.
- Delete the `chips.len() > 8` branch and the inline chip-strip branch.
- Every group builds `Vec<(Option<String>, Vec<NavLink>)>` and renders one
  `<GroupedNavAccordion>`:

  | Group | Sections |
  | --- | --- |
  | 1 Weapons | role buckets via `role_for_weapon_category` |
  | 5 Job Sets | role buckets via `role_for_job_abbr` |
  | 2 Armor, 3 Items, 4 Housing | `vec![(None, links)]` |

- The wrapper `<div>` loses its conditional class (`"flex"` vs
  `"item-explorer-chip-row"`) — the branch existed only because the chip
  row's mask gradient clipped the popover panel. It keeps its `role="navigation"`
  and `aria-label`.

### 4. Open state

```rust
// SSR and the first CSR render compute the same value.
let open = RwSignal::new(accordion_open_for_route(
    params.get_untracked().get("jobset").as_deref(),
    params.get_untracked().get("category").as_deref(),
));

// Pill click: switch group and reveal its chips. Does not navigate.
on:click = move |_| { selected_group.set(group); open.set(true); }

// Navigation: collapse onto a chosen category, re-open on bare /items.
// `active_group` is the existing Memo over the same two params, so
// `active_group.get().is_none()` is `accordion_open_for_route` applied to
// the current route — the same rule as the initialiser above.
Effect::new(move |_| {
    open.set(active_group.get().is_none());
});
```

`active_group` reads `params`, so tracking it is what makes the effect re-run
on navigation; no separate `pathname.track()` is needed.

**`active_group` must stay a `Memo`, not a `Signal::derive`.** The effect
relies on the memo's value diffing to *not* fire when the group is unchanged.
Category-to-category navigation within one group (`/items/category/24` →
`/items/category/25`) leaves the group at `Some(3)`; the memo stays quiet and
the accordion stays collapsed, which is what we want. Under `Signal::derive`
the effect would re-run on every param change — harmless here since it would
write the same `false`, but the gating is load-bearing enough to note, and a
future "optimise Memo → Signal::derive" pass must not touch this one.

The effect needs no first-run guard: on its initial run it computes the value
`open` was already initialised to, so there is no hydration flicker. Pill
clicks do not change the pathname, so they never re-trigger it and cannot be
undone by it.

The existing effect that syncs `selected_group` from the route stays as-is and
must not touch `open` — if it did, browser-back would expand the accordion.

### 5. Styling

Add accordion header and panel rules to `style/tailwind.css` beside the
existing `item-explorer-chip` block. Delete the `item-explorer-chip-row`
utility; the toolbar was its only consumer.

`item-explorer-chip` itself is unchanged. The chips are `<A>` (`<a>`) elements
that already render correctly under the global `a:not(...)` rule, so the
anchor-specificity hazard in that stylesheet is not in play here.

### 6. i18n

No new keys. `item_explorer_browse_subcategories` carries over as the header's
fallback label and `item_explorer_categories` as the nav landmark's
`aria-label`. `aria-expanded` conveys the expand/collapse affordance without a
visible string, so nothing new needs translating into all seven locales.

## Testing

`item_explorer_toolbar.rs` already unit-tests its route helpers as pure
functions with no router dependency. Extend that:

- Extract `accordion_open_for_route(jobset: Option<&str>, category: Option<&str>) -> bool`
  next to `active_group_from_route` and test the rule directly: open on
  `(None, None)`, closed for a jobset, closed for a category id, closed for a
  legacy percent-encoded category name, open for an unresolvable category.
- Assert every group `1..=5` produces at least one non-empty section. The
  deleted `group_uses_popover` branching implied this; nothing asserts it.
- Existing tests for `active_group_from_route`, `category_chips_for_group`,
  `job_chips_sorted`, and `job_chip_label` are unaffected and must stay green.

CI does not run `cargo test` for this repo, so the suite has to be run
locally before the PR — green CI only proves it compiles.

Manual check in a browser: switch pills (accordion opens each time), pick a
chip (collapses, header names the pick), reload the resulting deep link
(starts collapsed), navigate back to `/items` (re-opens).

## Accepted trade-offs

- **Job Sets expands tall.** Roughly 7 role sections and 40 chips. It flows
  naturally and scrolls with the page rather than being capped with an inner
  scroll region, because a nested scrollbar is the thing an accordion exists
  to avoid. On mobile this pushes the item list down while open — but it
  collapses on the first chip click, which is the path a user is on.
- **No category count in the header.** Considered and dropped; it would need
  a new i18n key with a plural form in seven locales for marginal value.

## Out of scope

Group pill styling, the world picker, `ItemList`, and the `/items` route
structure are untouched.
