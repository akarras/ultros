# Item Tooltip Hover Card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reusable `ItemTooltip` hover card on top of a rebuilt, bug-fixed `HoverCard` overlay primitive; restyle every text tooltip via the same primitive; slim the item page header.

**Architecture:** A new `HoverCard` primitive (`hover_card.rs`) owns the portal, hover/focus/Escape state, open-delay, and rewritten pure-viewport positioning (unit-tested pure function). `Tooltip` keeps its exact public API as a thin wrapper. New `ItemTooltip` renders an item card (icon/name/category/ilvl/stats/description) from `tracked_data()` — zero fetches. Wired into `SmallItemDisplay`, item explorer rows, and the item page header icon.

**Tech Stack:** Rust, Leptos 0.8 (nightly features), leptos-use, Tailwind (CSS-variable brand palettes), cargo test.

**Spec:** `docs/superpowers/specs/2026-07-31-item-tooltip-hover-card-design.md`

**All paths relative to the repo root.** The frontend crate is `ultros-frontend/ultros-app`.

---

### Task 1: Environment sanity check

The `ultros-app` crate's dependency `xiv-gen-db` needs git submodules to build. Tests and clippy will not run without them.

- [ ] **Step 1: Verify submodules are populated**

Run:
```bash
ls xiv-gen/ffxiv-datamining/csv/en/Item.csv xiv-gen/ffxiv-datamining/csv/cn/Item.csv xiv-gen/ffxiv-datamining/csv/ko/csv/Item.csv
ls ultros-frontend/ultros-xiv-icons/universalis-assets/icon2x | head -1
ls ultros/static/classjob-icons | wc -l
```

Expected: all files exist; the last count is non-zero.

- [ ] **Step 2: If anything is missing**, follow the `--reference` recipe in `CLAUDE.md` (section "When the submodule isn't initialized") with `MAIN=/Users/aaronkarras/code/ffxiv-playground`. Do NOT use `git submodule update --init --recursive --depth=1` — it fails on this repo (see CLAUDE.md for the failure modes). Re-run Step 1 to verify.

---

### Task 2: `overlay_position` pure function (TDD)

The heart of the bug fix: positioning math in pure viewport coordinates (the overlay is `position: fixed`), no scroll offsets anywhere. Extracted as a pure function so it's unit-testable.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/hover_card.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs` (add module, alphabetical order)

- [ ] **Step 1: Create the file with types, a stub, and failing tests**

Create `ultros-frontend/ultros-app/src/components/hover_card.rs`:

```rust
use leptos::prelude::*;

/// Anchor geometry in viewport coordinates (as returned by
/// `getBoundingClientRect`). The overlay is `position: fixed`, so all math in
/// this module stays in viewport space — no scroll offsets.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) struct AnchorRect {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) struct OverlaySize {
    pub width: f64,
    pub height: f64,
}

/// Minimum distance kept between the overlay and every viewport edge.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
const EDGE_MARGIN: f64 = 8.0;
/// Gap between the anchor and the overlay.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
const ANCHOR_GAP: f64 = 8.0;

/// Compute the `(top, left)` for a fixed-position overlay anchored to
/// `anchor`: centered above it, flipped below when there is no room above,
/// clamped to the viewport on both axes.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) fn overlay_position(
    anchor: AnchorRect,
    overlay: OverlaySize,
    viewport: OverlaySize,
) -> (f64, f64) {
    let _ = (anchor, overlay, viewport);
    todo!("implemented in the next step")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: OverlaySize = OverlaySize {
        width: 1280.0,
        height: 800.0,
    };
    const OVERLAY: OverlaySize = OverlaySize {
        width: 200.0,
        height: 100.0,
    };

    fn anchor(top: f64, left: f64) -> AnchorRect {
        AnchorRect {
            top,
            left,
            width: 40.0,
            height: 20.0,
        }
    }

    #[test]
    fn hover_card_positions_above_and_centered_when_there_is_room() {
        let (top, left) = overlay_position(anchor(400.0, 600.0), OVERLAY, VIEWPORT);
        // 8px above the anchor, horizontally centered on it.
        assert_eq!(top, 400.0 - 100.0 - 8.0);
        assert_eq!(left, 600.0 + 20.0 - 100.0);
    }

    #[test]
    fn hover_card_flips_below_when_no_room_above() {
        let (top, _) = overlay_position(anchor(50.0, 600.0), OVERLAY, VIEWPORT);
        assert_eq!(top, 50.0 + 20.0 + 8.0);
    }

    #[test]
    fn hover_card_clamps_to_left_edge() {
        let (_, left) = overlay_position(anchor(400.0, 4.0), OVERLAY, VIEWPORT);
        assert_eq!(left, 8.0);
    }

    #[test]
    fn hover_card_clamps_to_right_edge() {
        let (_, left) = overlay_position(anchor(400.0, 1270.0), OVERLAY, VIEWPORT);
        assert_eq!(left, 1280.0 - 200.0 - 8.0);
    }

    #[test]
    fn hover_card_flipped_overlay_near_bottom_is_clamped() {
        // Anchor near the top forces a flip below; the short viewport then
        // forces the vertical clamp so the overlay never overflows the bottom.
        let viewport = OverlaySize {
            width: 1280.0,
            height: 160.0,
        };
        let (top, _) = overlay_position(anchor(40.0, 600.0), OVERLAY, viewport);
        assert_eq!(top, 160.0 - 100.0 - 8.0);
    }

    #[test]
    fn hover_card_tiny_viewport_does_not_panic_and_pins_to_margin() {
        // Overlay bigger than the viewport: both clamp ranges collapse to the
        // edge margin instead of panicking (f64::clamp panics when min > max).
        let viewport = OverlaySize {
            width: 100.0,
            height: 60.0,
        };
        let (top, left) = overlay_position(anchor(10.0, 10.0), OVERLAY, viewport);
        assert_eq!(top, 8.0);
        assert_eq!(left, 8.0);
    }
}
```

(The `use leptos::prelude::*;` import is unused until Task 3 adds the component; if the compiler warns about it during this task, remove it here and re-add it in Task 3.)

Add to `ultros-frontend/ultros-app/src/components/mod.rs`, after `pub mod history_panel;` and before `pub mod icon;` (alphabetical):

```rust
pub mod hover_card;
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test -p ultros-app hover_card`
Expected: 6 tests FAIL (panic on `todo!`).

- [ ] **Step 3: Implement `overlay_position`**

Replace the stub body:

```rust
pub(crate) fn overlay_position(
    anchor: AnchorRect,
    overlay: OverlaySize,
    viewport: OverlaySize,
) -> (f64, f64) {
    // Prefer above the anchor; flip below when the overlay would clip the top.
    let mut top = anchor.top - overlay.height - ANCHOR_GAP;
    if top < EDGE_MARGIN {
        top = anchor.top + anchor.height + ANCHOR_GAP;
    }
    // `.max(EDGE_MARGIN)` keeps the clamp range valid when the overlay is
    // larger than the viewport (f64::clamp panics when min > max).
    let max_top = (viewport.height - overlay.height - EDGE_MARGIN).max(EDGE_MARGIN);
    let top = top.clamp(EDGE_MARGIN, max_top);

    let left = anchor.left + anchor.width / 2.0 - overlay.width / 2.0;
    let max_left = (viewport.width - overlay.width - EDGE_MARGIN).max(EDGE_MARGIN);
    let left = left.clamp(EDGE_MARGIN, max_left);

    (top, left)
}
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `cargo test -p ultros-app hover_card`
Expected: 6 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/hover_card.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(hover-card): add unit-tested overlay_position in pure viewport space"
```

---

### Task 3: `HoverCard` component + shared card chrome

The primitive: portal, hover/focus/Escape, open-delay, lazy measurement (no observers until open), no first-frame jump.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/hover_card.rs`

- [ ] **Step 1: Add imports, chrome constant, `AccentHairline`, and the component**

At the top of `hover_card.rs`, replace the existing `use leptos::prelude::*;` with:

```rust
use cfg_if::cfg_if;
use leptos::children::ViewFn;
use leptos::leptos_dom::helpers::{TimeoutHandle, set_timeout_with_handle};
#[cfg(feature = "hydrate")]
use leptos::portal::Portal;
use leptos::{html::Div, prelude::*};
#[cfg(feature = "hydrate")]
use leptos_use::{
    UseElementSizeReturn, UseEventListenerOptions, use_element_size,
    use_event_listener_with_options, use_window,
};
use std::time::Duration;
```

Below the `overlay_position` function (above the `tests` module), add:

```rust
/// Shared chrome for hover overlays: palette-driven gradient body, accent
/// hairline slot, glow shadow. Consumers append their own padding/sizing and
/// render `<AccentHairline/>` as their first child. Every color rides the
/// runtime brand CSS variables, so all palettes and light mode re-tint it.
pub(crate) const HOVER_CARD_CHROME: &str = "relative overflow-hidden rounded-lg \
    border border-brand-400/30 \
    bg-gradient-to-br from-brand-950/95 via-brand-900/90 to-brand-950/95 \
    backdrop-blur-md shadow-lg shadow-[color:var(--accent-glow)]";

/// 1px accent gradient across the top edge of a hover card.
#[component]
pub(crate) fn AccentHairline() -> impl IntoView {
    view! {
        <div class="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-[color:var(--accent)] to-transparent"></div>
    }
}

/// Hover/focus-triggered overlay primitive. Owns the portal, open/close state
/// (with optional open delay), and fixed positioning via [`overlay_position`].
/// No observers or listeners are created until the overlay actually opens.
#[component]
pub fn HoverCard<T>(
    /// Overlay content, rendered into a body portal while open.
    #[prop(into)]
    content: ViewFn,
    /// Milliseconds of sustained hover before opening. Focus opens instantly.
    #[prop(default = 0)]
    open_delay_ms: u32,
    /// While true, hover/focus never opens the overlay.
    #[prop(optional, into)]
    disabled: Signal<bool>,
    /// Classes for the anchor wrapper div.
    #[prop(optional, into)]
    class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send + 'static,
{
    let (hover_open, set_hover_open) = signal(false);
    let (is_focused, set_is_focused) = signal(false);
    // Pending open-delay timer. `new_local`: timers only exist client-side.
    let pending = StoredValue::new_local(None::<TimeoutHandle>);

    let clear_pending = move || {
        if let Some(handle) = pending.get_value() {
            handle.clear();
            pending.set_value(None);
        }
    };
    let request_open = move || {
        if disabled.get_untracked() {
            return;
        }
        if open_delay_ms == 0 {
            set_hover_open.set(true);
        } else if pending.get_value().is_none() {
            let handle = set_timeout_with_handle(
                move || {
                    pending.set_value(None);
                    set_hover_open.set(true);
                },
                Duration::from_millis(u64::from(open_delay_ms)),
            )
            .ok();
            pending.set_value(handle);
        }
    };

    let is_open = Signal::derive(move || !disabled.get() && (hover_open.get() || is_focused.get()));
    // Suppress unused warnings on the server build, where the overlay closure
    // below compiles to `None`.
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = is_open;
    }

    let target = NodeRef::<Div>::new();
    let content = content.clone();

    let overlay = {
        cfg_if! {
            if #[cfg(feature = "hydrate")] {
                let read_anchor_rect = move || {
                    target
                        .get_untracked()
                        .map(|el| {
                            let rect = el.get_bounding_client_rect();
                            AnchorRect {
                                top: rect.top(),
                                left: rect.left(),
                                width: rect.width(),
                                height: rect.height(),
                            }
                        })
                        .unwrap_or_default()
                };
                move || {
                    is_open.get().then({
                        let content = content.clone();
                        move || {
                            let anchor_rect = RwSignal::new(read_anchor_rect());
                            // Track the anchor while open: any scroll (capture
                            // catches nested containers) or resize moves its
                            // viewport rect. Registered inside the overlay
                            // view, so everything is dropped on close.
                            let _ = use_event_listener_with_options(
                                use_window(),
                                leptos::ev::scroll,
                                move |_| anchor_rect.set(read_anchor_rect()),
                                UseEventListenerOptions::default().capture(true).passive(true),
                            );
                            let _ = use_event_listener_with_options(
                                use_window(),
                                leptos::ev::resize,
                                move |_| anchor_rect.set(read_anchor_rect()),
                                UseEventListenerOptions::default().capture(false).passive(true),
                            );
                            let node_ref = NodeRef::<Div>::new();
                            let UseElementSizeReturn {
                                width: overlay_width,
                                height: overlay_height,
                            } = use_element_size(node_ref);
                            let style = move || {
                                let overlay = OverlaySize {
                                    width: overlay_width.get(),
                                    height: overlay_height.get(),
                                };
                                let viewport = OverlaySize {
                                    width: window()
                                        .inner_width()
                                        .ok()
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or_default(),
                                    height: window()
                                        .inner_height()
                                        .ok()
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or_default(),
                                };
                                let (top, left) =
                                    overlay_position(anchor_rect.get(), overlay, viewport);
                                // Keep hidden until measured so the first
                                // paint can't flash at the wrong position.
                                let visibility =
                                    if overlay.width == 0.0 && overlay.height == 0.0 {
                                        "visibility: hidden;"
                                    } else {
                                        ""
                                    };
                                format!("top: {top}px; left: {left}px; {visibility}")
                            };
                            view! {
                                <Portal mount=document().body().unwrap()>
                                    <div
                                        node_ref=node_ref
                                        role="tooltip"
                                        class="fixed z-50 transition-opacity duration-150 animate-fade-in"
                                        style=style
                                    >
                                        {content.run()}
                                    </div>
                                </Portal>
                            }
                            .into_any()
                        }
                    })
                }
            } else {
                {
                    let _ = content;
                    move || None::<AnyView>
                }
            }
        }
    };

    let children = children.into_inner();
    view! {
        <div
            class=class.unwrap_or_default()
            on:mouseenter=move |_| request_open()
            on:mouseleave=move |_| {
                clear_pending();
                set_hover_open.set(false);
            }
            on:focusin=move |_| set_is_focused.set(true)
            on:focusout=move |_| set_is_focused.set(false)
            on:keydown=move |ev| {
                if ev.key() == "Escape" {
                    clear_pending();
                    set_hover_open.set(false);
                    set_is_focused.set(false);
                }
            }
            node_ref=target
        >
            {children()}
            {overlay}
        </div>
    }
}
```

Note: `AnchorRect`, `OverlaySize`, `overlay_position`, `EDGE_MARGIN`, and `ANCHOR_GAP` are now referenced by the hydrate-only overlay closure — that is why they carry `#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]` rather than plain `#[allow(dead_code)]`. `HOVER_CARD_CHROME` and `AccentHairline` become used in Task 4.  Until then, mark them the same way temporarily OR proceed straight to Task 4 before running clippy — the recommended order is: finish this step, verify compilation with the command in Step 2, then do Task 4, then run the full `./check_ci.sh` once.

- [ ] **Step 2: Verify it compiles (both feature halves)**

Run:
```bash
cargo check -p ultros-app 2>&1 | tail -5
cargo check -p ultros-app --features hydrate 2>&1 | tail -5
```
Expected: both succeed (warnings about the not-yet-used `HOVER_CARD_CHROME`/`AccentHairline` are acceptable until Task 4; nothing else).

- [ ] **Step 3: Run the position tests again**

Run: `cargo test -p ultros-app hover_card`
Expected: 6 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/hover_card.rs
git commit -m "feat(hover-card): HoverCard primitive with lazy measurement and open delay"
```

---

### Task 4: Rewrite `Tooltip` as a thin wrapper (public API unchanged)

All ~20 call sites keep compiling untouched; they inherit the fixed positioning and the new theming. The old text color `text-gray-200` was unreadable in light mode (the brand scale flips); use `text-[color:var(--color-text)]` instead.

**Files:**
- Rewrite: `ultros-frontend/ultros-app/src/components/tooltip.rs`

- [ ] **Step 1: Replace the entire file content**

```rust
use leptos::prelude::*;

use super::hover_card::{AccentHairline, HOVER_CARD_CHROME, HoverCard};

/// Plain-text tooltip. Thin wrapper over [`HoverCard`] — same public API as
/// the original standalone implementation.
#[component]
pub fn Tooltip<T>(
    #[prop(into)] tooltip_text: Signal<String>,
    #[prop(optional, into)] class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send + 'static,
{
    let disabled = Signal::derive(move || tooltip_text.with(|t| t.is_empty()));
    let children = children.into_inner();
    view! {
        <HoverCard
            disabled=disabled
            class=format!("inline-block {}", class.unwrap_or_default())
            content=move || {
                view! {
                    <div class=format!(
                        "{HOVER_CARD_CHROME} px-4 py-2 text-sm text-[color:var(--color-text)]",
                    )>
                        <AccentHairline />
                        {move || tooltip_text.get()}
                    </div>
                }
            }
        >
            {move || children()}
        </HoverCard>
    }
}
```

This deletes the old `use_window_size` helper and the old positioning code entirely. If the original `Tooltip` bounds (`T: Sized + Render + RenderHtml + Send`, without `'static`) cause an error against `HoverCard`'s `'static` bound, keep `'static` on both — `TypedChildrenFn` values are `'static` in practice, so no call site changes.

- [ ] **Step 2: Full CI check (this compiles every Tooltip call site)**

Run: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: `REAL_EXIT=0`. (Exit `137` means clippy was OOM-killed, not a lint failure — re-run clippy as `cargo clippy --all-targets -j 2 -- -D warnings`.)

- [ ] **Step 3: Run all crate tests**

Run: `cargo test -p ultros-app`
Expected: all tests PASS (hover_card's 6 plus pre-existing suites).

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/tooltip.rs
git commit -m "refactor(tooltip): rebuild on HoverCard, fix positioning bugs, retheme"
```

---

### Task 5: `ItemTooltip` component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/item_tooltip.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs` (add module, alphabetical order)

- [ ] **Step 1: Create the component**

Create `ultros-frontend/ultros-app/src/components/item_tooltip.rs`:

```rust
use leptos::prelude::*;
use xiv_gen::{ItemId, ItemUiCategoryId};

use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;

use super::hover_card::{AccentHairline, HOVER_CARD_CHROME, HoverCard};
use super::item_icon::{IconSize, ItemIcon};
use super::stats_display::ItemStats;
use super::ui_text::UIText;

/// Sustained-hover delay before the card opens, so sweeping the cursor across
/// item tables doesn't strobe cards.
const OPEN_DELAY_MS: u32 = 300;

/// Wraps any item surface (row, icon, link) with a hover card showing the
/// item's icon, name, category, item level, stats, and description. All data
/// comes synchronously from `tracked_data()` — no fetches. Unknown ids render
/// the children with hover disabled.
#[component]
pub fn ItemTooltip<T>(
    #[prop(into)] item_id: Signal<i32>,
    /// Classes for the anchor wrapper div (use to preserve the layout the
    /// wrapped content expects, e.g. flex row classes).
    #[prop(optional, into)]
    class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send + 'static,
{
    let i18n = use_i18n();
    let disabled =
        Signal::derive(move || !tracked_data().items.contains_key(&ItemId(item_id.get())));
    let content = move || {
        let data = tracked_data();
        let Some(item) = data.items.get(&ItemId(item_id.get_untracked())) else {
            return ().into_any();
        };
        let category = data
            .item_ui_categorys
            .get(&ItemUiCategoryId(item.item_ui_category))
            .map(|category| category.name.as_str());
        view! {
            <div class=format!("{HOVER_CARD_CHROME} w-max max-w-sm p-4 flex flex-col gap-3")>
                <AccentHairline />
                <div class="flex items-center gap-3">
                    <div class="relative shrink-0">
                        // Soft palette-tinted bloom behind the icon.
                        <div class="absolute -inset-2 rounded-full bg-[radial-gradient(circle,var(--accent-glow),transparent_70%)]"></div>
                        <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium />
                    </div>
                    <div class="flex flex-col min-w-0 flex-1">
                        <span class="font-bold text-[color:var(--color-text)] leading-tight">
                            {item.name.as_str()}
                        </span>
                        {category
                            .map(|name| {
                                view! { <span class="text-sm text-brand-300">{name}</span> }
                            })}
                    </div>
                    <div class="flex items-center gap-1.5 shrink-0 self-start">
                        <span class="text-brand-300 font-medium tracking-wide text-xs uppercase">
                            {t_string!(i18n, item_level).to_string()}
                        </span>
                        <span class="text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-400/50">
                            {item.level_item}
                        </span>
                    </div>
                </div>
                <ItemStats item_id=item.key_id />
                {(!item.description.is_empty())
                    .then(|| {
                        view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] line-clamp-3">
                                <UIText text=item.description.as_str().to_string() />
                            </div>
                        }
                    })}
            </div>
        }
        .into_any()
    };
    let children = children.into_inner();
    view! {
        <HoverCard
            content=content
            disabled=disabled
            open_delay_ms=OPEN_DELAY_MS
            class=class.unwrap_or_default()
        >
            {move || children()}
        </HoverCard>
    }
}
```

Note: `item.description.is_empty()` / `.as_str()` — if `description`'s type differs (e.g. needs `item.description.as_str().is_empty()`), match whatever `routes/item_view.rs:1719` (`item_description`) compiles with today.

Add to `ultros-frontend/ultros-app/src/components/mod.rs` after `pub mod item_icon;`:

```rust
pub mod item_tooltip;
```

- [ ] **Step 2: Verify compilation**

Run:
```bash
cargo check -p ultros-app 2>&1 | tail -5
cargo check -p ultros-app --features hydrate 2>&1 | tail -5
```
Expected: success. (An unused-component warning is possible until Task 6 wires it in; if `-D warnings` complains in check_ci later, Task 6 resolves it — run Tasks 5 and 6 back-to-back.)

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/item_tooltip.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(item-tooltip): reusable item hover card over HoverCard"
```

---

### Task 6: Wire into `SmallItemDisplay`

This one change covers the analyzers, lists, related items, live sale ticker, and every other `SmallItemDisplay` consumer.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/small_item_display.rs`

- [ ] **Step 1: Wrap the display in `ItemTooltip`**

Replace the `SmallItemDisplay` component (keep `ItemDetails` as is):

```rust
#[component]
pub fn SmallItemDisplay(item: &'static Item) -> impl IntoView {
    let (price_zone, _) = get_price_zone();
    view! {
        <ItemTooltip
            item_id=item.key_id.0
            class="flex flex-row items-center gap-2 min-w-0"
        >
            // If the item isn't marketable then do not display a market link
            {if item.item_search_category == 0 {
                Either::Left(view! { <ItemDetails item /> })
            } else {
                Either::Right(
                    view! {
                        <A
                            attr:class="flex flex-row items-center gap-2 min-w-0"
                            exact=true
                            href=move || {
                                format!(
                                    "/item/{}/{}",
                                    price_zone()
                                        .as_ref()
                                        .map(|z| z.get_name())
                                        .unwrap_or("North-America"),
                                    item.key_id.0,
                                )
                            }
                        >
                            <ItemDetails item />
                        </A>
                    },
                )
            }}

        </ItemTooltip>
    }
    .into_any()
}
```

Add the import at the top of the file:

```rust
use super::item_tooltip::ItemTooltip;
```

The `ItemTooltip` anchor div takes over the old outer div's classes (`flex flex-row items-center gap-2 min-w-0`), so the layout inside virtual-scroller rows and flex tables is unchanged.

- [ ] **Step 2: CI check + tests**

Run: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: `REAL_EXIT=0`.
Run: `cargo test -p ultros-app`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/small_item_display.rs
git commit -m "feat(item-tooltip): show hover card on SmallItemDisplay surfaces"
```

---

### Task 7: Wire into item explorer rows

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer.rs` (the `<For>` row around line 736–764)

- [ ] **Step 1: Wrap the icon link cell**

In the row `children` closure (`children=move |(id, item)| { let item_id = id.0; ... }`), the first grid cell is:

```rust
<A href=move || format!("/item/{}/{}",
    scope_name.get(),
    item.key_id.0)
>
    <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small />
</A>
```

Wrap it (the row already binds `let item_id = id.0;` — use that, keeping the anchor cell a plain grid child):

```rust
<ItemTooltip item_id=item_id>
    <A href=move || format!("/item/{}/{}",
        scope_name.get(),
        item_id)
    >
        <ItemIcon item_id=item_id icon_size=IconSize::Small />
    </A>
</ItemTooltip>
```

Add the import at the top of the file alongside the other component imports (match the file's existing import style, e.g.):

```rust
use crate::components::item_tooltip::ItemTooltip;
```

- [ ] **Step 2: CI check**

Run: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: `REAL_EXIT=0`.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_explorer.rs
git commit -m "feat(item-tooltip): hover card on item explorer row icons"
```

---

### Task 8: Item page — hover card on header icon, drop description row

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs`

- [ ] **Step 1: Wrap the header icon**

At `routes/item_view.rs:1770`, change:

```rust
<ItemIcon item_id icon_size=IconSize::Large />
```

to:

```rust
<ItemTooltip item_id=item_id>
    <ItemIcon item_id icon_size=IconSize::Large />
</ItemTooltip>
```

(`item_id` in this scope is already a reactive `i32` signal/memo; `#[prop(into)]` accepts it.) Add the import near the other `crate::components::` imports at the top of the file:

```rust
use crate::components::item_tooltip::ItemTooltip;
```

- [ ] **Step 2: Remove the description row from the header grid**

Delete this block (around `routes/item_view.rs:1836-1841`):

```rust
<div
    class="lg:col-span-2 text-sm sm:text-base text-[color:var(--color-text-muted)] line-clamp-3"
    class:hidden=move || { item_description().is_empty() }
>
    {move || view! { <UIText text=item_description().to_string() /> }}
</div>
```

Then delete the now-unused `item_description` closure (around line 1719):

```rust
let item_description = move || {
    tracked_data()
        .items
        .get(&ItemId(item_id()))
        .map(|item| item.description.as_str())
        .unwrap_or_default()
        .to_string()
};
```

If `UIText` has no remaining uses in this file, remove its import too (clippy/`-D warnings` will flag it either way). The ilvl chip and `ItemStats` grid stay exactly as they are. The SEO meta description (`item_view_meta_description`) is unrelated to this block and stays.

- [ ] **Step 3: CI check + tests**

Run: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: `REAL_EXIT=0`.
Run: `cargo test -p ultros-app`
Expected: PASS (this file has its own test module — must stay green).

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "feat(item-view): hover card on header icon, drop inline description"
```

---

### Task 9: End-to-end verification

- [ ] **Step 1: Full CI gate one last time**

Run: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: `REAL_EXIT=0`.

- [ ] **Step 2: E2E smoke + screenshots**

Run: `./scripts/run_e2e.sh`
Expected: exit 0. This builds the app, boots it on a random port, and runs the Puppeteer screenshot suite in `integration/`.

- [ ] **Step 3: Manual hover verification in a browser**

With a dev server up (reuse the e2e server via `REUSE_SERVER=1` or start one), verify in the browser:

1. Item page (e.g. `/item/North-America/34215`): header shows NO description text; hovering the header icon for ~300ms opens the card with icon, name, category, ilvl chip, stats, clamped description.
2. Scroll down the item page to the related-items section, hover a related item row: card opens ABOVE the row when there is room (this is the scrolled-page regression the old code failed), flips below near the viewport top, never overflows an edge.
3. A plain text tooltip (e.g. the copy-name button next to the item title) shows the new gradient + accent hairline styling and appears without delay.
4. Switch palette (theme picker) to at least one FFXIV palette and to light mode: card gradient, hairline, and glow re-tint; text stays readable in light mode.
5. Item explorer (`/items/category/Marauder%27s%20Arm` or any category): hovering a row icon opens the card; sweeping the mouse quickly across rows does NOT flash cards (300ms delay).
6. Press Escape while a card is open: it closes.

- [ ] **Step 4: Update the plan checkboxes and stop**

Implementation complete. Hand back for review/merge per the finishing-a-development-branch skill.

---

## Notes for the implementer

- **Never commit without** `./check_ci.sh` passing (see CLAUDE.md; read the exit code with `; echo "REAL_EXIT=$?"`, not through a pipe).
- **No new i18n keys are expected.** Item names, categories, stats, and descriptions come from game data; the `item_level` key already exists in all locales. If you find yourself adding a string literal to a `view!`, stop and route it through `leptos-i18n` per CLAUDE.md (all 7 locale files).
- **Tailwind classes must appear literally in source** — do not build class names with runtime string concatenation beyond joining the literal constants shown here.
- `ViewFn` is `leptos::children::ViewFn` (used by e.g. `Show`'s `fallback`); `From<F: Fn() -> impl RenderHtml>` gives the `#[prop(into)]` conversion.
- `set_timeout_with_handle` / `TimeoutHandle` live in `leptos::leptos_dom::helpers` (the codebase already uses `set_timeout` from there).
