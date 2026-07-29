// ultros-frontend/ultros-app/src/components/on_hand_input.rs
//! LocalStorage-backed on-hand tracking. Consumed by the analyzer routes in Tasks 7-9.
// Dead-code allow retained: STORAGE_KEY / from_storage are cfg-gated non-SSR so the
// lib/ssr build never sees them; ListOnHand / from_items are pub API consumed by
// hydrate-feature callers; Leptos component prop structs appear unused to clippy in
// the lib target. The follow-up PR that plumbs the live-list resource fetch can
// narrow or remove this allow once ListOnHand gains a concrete call site in SSR.
#![allow(dead_code)]

use crate::components::crafting_cost::OnHand;
use crate::i18n::{t, t_string, use_i18n};
use leptos::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

const STORAGE_KEY: &str = "ultros.craft.on_hand.v1";

/// LocalStorage-backed OnHand. Reads/writes a JSON HashMap<item_id, qty>.
///
/// Interior mutability: each `compute_cost` call consumes from a per-call
/// snapshot held in a RefCell, so two ingredient lines for the same
/// item share the same pool. Mutations are NOT persisted back to
/// storage — the user owns the canonical qty via the UI.
pub struct LocalOnHand {
    snapshot: RefCell<HashMap<i32, i32>>,
}

impl LocalOnHand {
    /// Take a fresh snapshot from LocalStorage. Call at the top of each
    /// reactive `compute_cost` derivation.
    pub fn from_storage() -> Self {
        let snapshot = read_storage().unwrap_or_default();
        Self {
            snapshot: RefCell::new(snapshot),
        }
    }

    /// Construct from an explicit map (tests + ListOnHand backfill).
    pub fn from_map(map: HashMap<i32, i32>) -> Self {
        Self {
            snapshot: RefCell::new(map),
        }
    }
}

impl OnHand for LocalOnHand {
    fn available(&self, item: ItemId) -> i32 {
        self.snapshot.borrow().get(&item.0).copied().unwrap_or(0)
    }
    fn consume(&self, item: ItemId, qty: i32) {
        let mut s = self.snapshot.borrow_mut();
        if let Some(v) = s.get_mut(&item.0) {
            *v = (*v - qty).max(0);
        }
    }
}

/// Reads on-hand from the user's active list's `ListItem.acquired` field.
/// Snapshotted at construction; `consume` mutates the snapshot only
/// (the list page is the canonical write path for `acquired`).
pub struct ListOnHand {
    snapshot: RefCell<HashMap<i32, i32>>,
    pub list_id: i32,
    pub list_name: String,
}

impl ListOnHand {
    pub fn from_items(list_id: i32, list_name: String, items: &[ListItem]) -> Self {
        let snapshot = items
            .iter()
            .filter_map(|i| i.acquired.map(|q| (i.item_id, q)))
            .collect();
        Self {
            snapshot: RefCell::new(snapshot),
            list_id,
            list_name,
        }
    }
}

impl OnHand for ListOnHand {
    fn available(&self, item: ItemId) -> i32 {
        self.snapshot.borrow().get(&item.0).copied().unwrap_or(0)
    }
    fn consume(&self, item: ItemId, qty: i32) {
        let mut s = self.snapshot.borrow_mut();
        if let Some(v) = s.get_mut(&item.0) {
            *v = (*v - qty).max(0);
        }
    }
}

fn read_storage() -> Option<HashMap<i32, i32>> {
    #[cfg(not(feature = "ssr"))]
    {
        let win = web_sys::window()?;
        let storage = win.local_storage().ok()??;
        let raw = storage.get_item(STORAGE_KEY).ok()??;
        serde_json::from_str(&raw).ok()
    }
    #[cfg(feature = "ssr")]
    {
        None
    }
}

fn write_storage(map: &HashMap<i32, i32>) {
    #[cfg(not(feature = "ssr"))]
    {
        if let Some(win) = web_sys::window() {
            if let Ok(Some(storage)) = win.local_storage() {
                if let Ok(s) = serde_json::to_string(map) {
                    let _ = storage.set_item(STORAGE_KEY, &s);
                }
            }
        }
    }
    #[cfg(feature = "ssr")]
    {
        let _ = map;
    }
}

/// Global reactive on-hand map. Mounted once via provide_on_hand_context.
/// Components that need to display or write reactively use this signal.
#[derive(Clone, Copy)]
pub struct OnHandMap(pub RwSignal<HashMap<i32, i32>>);

/// Call once at app startup (in AppInner) to provide the OnHandMap context
/// and wire up localStorage persistence. Idempotent — calling more than once
/// (e.g. via a nested `OnHandProvider`) is a no-op.
pub fn provide_on_hand_context() {
    if use_context::<OnHandMap>().is_some() {
        return;
    }
    let initial = read_storage().unwrap_or_default();
    let sig = RwSignal::new(initial);
    Effect::new(move |_| {
        sig.with(write_storage);
    });
    provide_context(OnHandMap(sig));
}

/// Collapsible component wrapper — call `provide_on_hand_context()` at the
/// app root and use `OnHandProvider` only if you need a wrapping element.
#[component]
pub fn OnHandProvider(children: Children) -> impl IntoView {
    provide_on_hand_context();
    children()
}

/// Inline per-ingredient quantity input.
///
/// Pass `item_name` so the screen-reader label disambiguates between
/// multiple inputs in the same recipe panel.
#[component]
pub fn OnHandQuantity(
    #[prop(into)] item_id: Signal<i32>,
    #[prop(into, optional)] item_name: Option<Signal<String>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let on_hand = use_context::<OnHandMap>().expect("OnHandMap not provided");
    let value = Signal::derive(move || on_hand.0.with(|m| m.get(&item_id()).copied().unwrap_or(0)));
    let aria = move || match &item_name {
        Some(name) => format!("On-hand quantity for {}", name.get()),
        None => "On-hand quantity".to_string(),
    };

    view! {
        <input
            type="number"
            min="0"
            class="input input-xs w-20 text-right"
            placeholder=t_string!(i18n, on_hand_placeholder_zero)
            aria-label=aria
            prop:value=move || value().to_string()
            on:input=move |ev| {
                let raw = event_target_value(&ev);
                let parsed: i32 = raw.parse().unwrap_or(0).max(0);
                let id = item_id();
                on_hand.0.update(|m| {
                    if parsed == 0 {
                        m.remove(&id);
                    } else {
                        m.insert(id, parsed);
                    }
                });
            }
        />
    }
}

/// Collapsible global panel listing every tracked item, with a reset button.
/// Mounted on the analyzer routes.
#[component]
pub fn OnHandPanel() -> impl IntoView {
    let i18n = use_i18n();
    let on_hand = use_context::<OnHandMap>().expect("OnHandMap not provided");
    let is_empty = Signal::derive(move || on_hand.0.with(|m| m.is_empty()));

    view! {
        <div class="panel p-4 rounded-lg border border-brand-700/30">
            <div class="flex flex-row items-center justify-between mb-2">
                <h3 class="font-bold text-brand-200">{t!(i18n, on_hand_heading)}</h3>
                <button
                    class="btn-ghost text-xs"
                    on:click=move |_| on_hand.0.update(|m| m.clear())
                    disabled=is_empty
                >
                    "Reset"
                </button>
            </div>
            <Show
                when=move || !is_empty()
                fallback=|| view! {
                    <div class="text-xs text-[color:var(--color-text-muted)]">
                        "Set on-hand counts on individual ingredient rows."
                    </div>
                }
            >
                {move || {
                    let count = on_hand.0.with(|m| m.len());
                    view! {
                        <div class="text-xs text-[color:var(--color-text-muted)]">
                            {format!("{} items tracked", count)}
                        </div>
                    }
                }}
            </Show>
        </div>
    }
}

/// Banner shown on cost-quoting surfaces when an active craft list is set.
/// Renders a one-liner indicating on-hand data comes from the active list,
/// plus a "Detach" button that clears `active_craft_list` in the cookie.
/// Returns nothing when no active list is set.
#[component]
pub fn ActiveListBanner() -> impl IntoView {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::{self, CraftOptions};
    let cookies = use_context::<Cookies>().unwrap();
    let (opts_signal, set_opts) =
        cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);

    let active_id = move || opts_signal.get().unwrap_or_default().active_craft_list;

    move || {
        active_id().map(|id| {
            view! {
                <div class="panel p-2 rounded-md border border-emerald-700/30 bg-emerald-900/10 flex flex-row items-center justify-between text-sm">
                    <span class="text-emerald-300">
                        "On-hand pulled from list #" {id}
                    </span>
                    <button
                        class="btn-ghost text-xs"
                        on:click=move |_| {
                            let mut o = opts_signal.get().unwrap_or_default();
                            o.active_craft_list = None;
                            set_opts(Some(o));
                        }
                    >
                        "Detach"
                    </button>
                </div>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn local_on_hand_from_map_basic() {
        let mut m = HashMap::new();
        m.insert(100, 5);
        let oh = LocalOnHand::from_map(m);
        assert_eq!(oh.available(ItemId(100)), 5);
        assert_eq!(oh.available(ItemId(999)), 0);
    }

    #[test]
    fn local_on_hand_consume_decrements() {
        let mut m = HashMap::new();
        m.insert(100, 5);
        let oh = LocalOnHand::from_map(m);
        oh.consume(ItemId(100), 3);
        assert_eq!(oh.available(ItemId(100)), 2);
    }

    #[test]
    fn local_on_hand_consume_clamps_at_zero() {
        let mut m = HashMap::new();
        m.insert(100, 2);
        let oh = LocalOnHand::from_map(m);
        oh.consume(ItemId(100), 99);
        assert_eq!(oh.available(ItemId(100)), 0);
    }
}
