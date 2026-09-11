//! The redesigned Build cart (Track B of #1427), mounted behind the
//! `lists-sync` Labs experiment for account and device lists alike.
//!
//! Presentation only: the [`ListWorkspaceSource`] it consumes carries the
//! rows, capabilities and edit callbacks, and the route that built the
//! source keeps the document, sync, authorization and guest storage
//! lifecycles. Each part of the cart lives in its own file so the follow-on
//! tickets can change rows, summary, details, selection and feedback
//! independently. The legacy Labs grid, `ListBuildWorkspace`, stays in
//! `routes::list_view_sync` untouched.

pub mod details;
pub mod estimate;
pub mod row;
pub mod selection;
pub mod summary;

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use crate::components::list::filter_row::{SortKey, SortSpec};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::list_view::remaining_quantity;
use crate::routes::list_view_sync::{InlineListAdd, InlineRecipeAdd, ListWorkspaceSource};

use row::{CartRow, ROW_GRID};
use selection::{CartSelectionBar, retain_present};
use summary::CartSummary;

/// `?cart=legacy` mounts the previous Labs grid (`ListBuildWorkspace`) so a
/// tester can compare the two presentations on the same list. The
/// redesigned cart is the default under the experiment; no cookie or Labs
/// token changes here.
pub fn use_legacy_cart() -> Signal<bool> {
    let query = ultros_ui::components::app_link::use_query_map_or_default();
    Memo::new(move |_| query.with(|q| q.get("cart").is_some_and(|v| v == "legacy"))).into()
}

/// Deterministic, total ordering for the cart. `Price` sorts by the line
/// estimate (what the column shows), unknown lines last; equal keys fall
/// back to the row id so the order can never surface map iteration order.
pub fn sort_cart_rows<'a>(
    rows: &mut [(ListItem, Vec<ActiveListing>)],
    spec: SortSpec,
    name_of: impl Fn(i32) -> Option<&'a str>,
) {
    rows.sort_by(|(a, a_listings), (b, b_listings)| {
        let ordering = match spec.key {
            SortKey::Name => name_of(a.item_id)
                .unwrap_or_default()
                .cmp(name_of(b.item_id).unwrap_or_default()),
            SortKey::Price => {
                let cost = |item: &ListItem, listings: &[ActiveListing]| {
                    estimate::estimate_line(item, listings)
                        .total
                        .unwrap_or(i64::MAX)
                };
                cost(a, a_listings).cmp(&cost(b, b_listings))
            }
            SortKey::Acquired => remaining_quantity(a).cmp(&remaining_quantity(b)),
        };
        let ordering = if spec.descending {
            ordering.reverse()
        } else {
            ordering
        };
        ordering.then_with(|| a.id.cmp(&b.id))
    });
}

/// The `sort` query value for a header click: first click sorts ascending,
/// a second click on the same column flips it, a third clears it.
pub fn next_sort(current: Option<SortSpec>, key: SortKey) -> Option<SortSpec> {
    match current {
        Some(spec) if spec.key == key && !spec.descending => Some(SortSpec {
            key,
            descending: true,
        }),
        Some(spec) if spec.key == key => None,
        _ => Some(SortSpec {
            key,
            descending: false,
        }),
    }
}

fn sort_option(spec: Option<SortSpec>) -> &'static str {
    match spec {
        None => "",
        Some(SortSpec {
            key: SortKey::Name,
            descending: false,
        }) => "name",
        Some(SortSpec {
            key: SortKey::Name,
            descending: true,
        }) => "name-desc",
        Some(SortSpec {
            key: SortKey::Acquired,
            descending: false,
        }) => "acquired",
        Some(SortSpec {
            key: SortKey::Acquired,
            descending: true,
        }) => "acquired-desc",
        Some(SortSpec {
            key: SortKey::Price,
            descending: false,
        }) => "price",
        Some(SortSpec {
            key: SortKey::Price,
            descending: true,
        }) => "price-desc",
    }
}

fn parse_sort_option(value: &str) -> Option<SortSpec> {
    let (key, descending) = match value.strip_suffix("-desc") {
        Some(key) => (key, true),
        None => (value, false),
    };
    let key = match key {
        "name" => SortKey::Name,
        "acquired" => SortKey::Acquired,
        "price" => SortKey::Price,
        _ => return None,
    };
    Some(SortSpec { key, descending })
}

/// The list is mounted once, independently of resource revisions. Row
/// identity is the document row key; each cell reads its current value
/// from its own memo, so a remote update never rebuilds an editor.
#[component]
pub fn ListCart(
    source: ListWorkspaceSource,
    selected_items: RwSignal<HashSet<i32>>,
    #[prop(default = Signal::derive(HashSet::new))] highlighted: Signal<HashSet<i32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let filter = RwSignal::new(String::new());
    // Row ids with an open details panel. Keyed by id, not position.
    let expanded: RwSignal<HashSet<i32>> = RwSignal::new(HashSet::new());
    // While an editor inside the list has focus, rows keep their order and
    // an acquired row stays visible, so committing an edit never moves the
    // control the player is typing in — including under an active sort.
    let editing = RwSignal::new(false);
    let grid = NodeRef::<leptos::html::Div>::new();
    let visible = Memo::new(
        move |previous: Option<&Vec<(ListItem, Vec<ActiveListing>)>>| {
            let query = filter.get().to_lowercase();
            let data = tracked_data();
            let pinned: HashSet<i32> = if editing.get() {
                previous
                    .into_iter()
                    .flatten()
                    .map(|(item, _)| item.id)
                    .collect()
            } else {
                HashSet::new()
            };
            let mut rows = source
                .rows
                .get()
                .into_iter()
                .filter(|(item, _)| {
                    !source.hide_acquired.get()
                        || remaining_quantity(item) > 0
                        || pinned.contains(&item.id)
                })
                .filter(|(item, _)| {
                    query.is_empty()
                        || data
                            .items
                            .get(&ItemId(item.item_id))
                            .is_some_and(|i| i.name.to_lowercase().contains(&query))
                        || item.item_id.to_string().contains(&query)
                })
                .collect::<Vec<_>>();
            if let Some(spec) = source.sort.get() {
                sort_cart_rows(&mut rows, spec, |id| {
                    data.items.get(&ItemId(id)).map(|item| item.name.as_str())
                });
            }
            if editing.get()
                && let Some(previous) = previous
            {
                let positions: HashMap<_, _> = previous
                    .iter()
                    .enumerate()
                    .map(|(index, (item, _))| (item.id, index))
                    .collect();
                rows.sort_by_key(|(item, _)| {
                    positions.get(&item.id).copied().unwrap_or(usize::MAX)
                });
            }
            rows
        },
    );
    let visible_ids = Memo::new(move |_| {
        visible.with(|rows| rows.iter().map(|(item, _)| item.id).collect::<Vec<_>>())
    });
    // A row that left the document (deleted here, by a collaborator or by
    // undo) leaves the selection and the open-panel set with it.
    Effect::new(move |_| {
        let present: HashSet<i32> = source
            .rows
            .with(|rows| rows.iter().map(|(item, _)| item.id).collect());
        if selected_items.with_untracked(|s| s.iter().any(|id| !present.contains(id))) {
            selected_items.update(|s| {
                retain_present(s, |id| present.contains(&id));
            });
        }
        if expanded.with_untracked(|s| s.iter().any(|id| !present.contains(id))) {
            expanded.update(|s| {
                retain_present(s, |id| present.contains(&id));
            });
        }
    });
    let is_empty = Memo::new(move |_| source.rows.with(|rows| rows.is_empty()));
    let all_visible_selected = Memo::new(move |_| {
        let ids = visible_ids.get();
        !ids.is_empty() && selected_items.with(|s| ids.iter().all(|id| s.contains(id)))
    });
    let sort_header = move |key: SortKey, label: Signal<String>, align_right: bool| {
        let active = Memo::new(move |_| source.sort.get().filter(|spec| spec.key == key));
        view! {
            <div role="columnheader" class=if align_right { "text-right" } else { "" } aria-sort=move || match active.get() {
                Some(spec) if spec.descending => "descending",
                Some(_) => "ascending",
                None => "none",
            }>
                <button type="button" class="inline-flex items-center gap-1 rounded px-1 uppercase tracking-wide hover:text-[color:var(--color-text)]" class:text-brand-300=move || active.get().is_some() aria-label=move || t_string!(i18n, cart_sort_column, column = label.get()).to_string() on:click=move |_| source.set_sort.run(next_sort(source.sort.get_untracked(), key))>
                    <span>{move || label.get()}</span>
                    <span aria-hidden="true">{move || match active.get() { Some(spec) if spec.descending => "▼", Some(_) => "▲", None => "" }}</span>
                </button>
            </div>
        }
    };
    let item_label = Signal::derive(move || t_string!(i18n, lists_workspace_item).to_string());
    let qty_label = Signal::derive(move || t_string!(i18n, cart_qty).to_string());
    let cost_label = Signal::derive(move || t_string!(i18n, cart_est_cost).to_string());
    view! {
        <section class="space-y-3" data-testid="list-cart">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=source.add pending=source.pending feedback=source.feedback />
                <div class="flex flex-wrap items-center gap-2 text-sm">
                    <button type="button" class="btn-ghost" aria-expanded=move || source.recipe_open.get().to_string() on:click=move |_| source.toggle_recipe.run(())>{t!(i18n, lists_workspace_add_recipe)}</button>
                    <span class="mx-1 h-4 border-l border-[color:var(--color-outline)]" aria-hidden="true"></span>
                    <button type="button" class="btn-ghost disabled:cursor-not-allowed disabled:opacity-40" data-testid="list-undo" disabled=move || !source.can_undo.get() on:click=move |_| source.undo.run(())>{t!(i18n, lists_workspace_undo)}</button>
                    <button type="button" class="btn-ghost disabled:cursor-not-allowed disabled:opacity-40" data-testid="list-redo" disabled=move || !source.can_redo.get() on:click=move |_| source.redo.run(())>{t!(i18n, lists_workspace_redo)}</button>
                </div>
                <Show when=move || source.recipe_open.get()><InlineRecipeAdd list_id=source.list_id on_add=source.add_many /></Show>
            </Show>
            <CartSummary rows=source.rows />
            <div class="flex flex-wrap items-center gap-2">
                <input class="input min-w-0 flex-1" type="search" aria-label=t_string!(i18n, lists_workspace_filter_label) placeholder=t_string!(i18n, lists_workspace_filter_placeholder) prop:value=move || filter.get() on:input=move |ev| filter.set(event_target_value(&ev)) />
                <label class="flex items-center gap-2 text-sm sm:hidden">
                    <span class="text-[color:var(--color-text-muted)]">{t!(i18n, cart_sort_by)}</span>
                    <select class="input" aria-label=t_string!(i18n, cart_sort_by) prop:value=move || sort_option(source.sort.get()) on:change=move |ev| source.set_sort.run(parse_sort_option(&event_target_value(&ev)))>
                        <option value="">{t!(i18n, cart_sort_default)}</option>
                        <option value="name">{t!(i18n, cart_sort_name_asc)}</option>
                        <option value="name-desc">{t!(i18n, cart_sort_name_desc)}</option>
                        <option value="acquired">{t!(i18n, cart_sort_qty_asc)}</option>
                        <option value="acquired-desc">{t!(i18n, cart_sort_qty_desc)}</option>
                        <option value="price">{t!(i18n, cart_sort_cost_asc)}</option>
                        <option value="price-desc">{t!(i18n, cart_sort_cost_desc)}</option>
                    </select>
                </label>
            </div>
            <CartSelectionBar selected_items visible_ids=visible_ids.into() can_write=source.can_write pending=source.bulk_pending on_remove_many=source.remove_many on_set_quality=source.set_quality_many />
            <div class="panel rounded-xl" node_ref=grid on:focusin=move |_| editing.set(true) on:focusout=move |ev| {
                #[cfg(feature = "hydrate")]
                {
                use wasm_bindgen::JsCast;
                let inside = ev.related_target().and_then(|target| target.dyn_into::<web_sys::Node>().ok()).is_some_and(|target| grid.get().is_some_and(|grid| grid.contains(Some(&target))));
                if !inside { editing.set(false); }
                }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            }>
                <div role="row" class=format!("{ROW_GRID} hidden sm:grid border-b border-[color:var(--color-outline)] text-xs text-[color:var(--color-text-muted)]")>
                    <div role="columnheader" class="justify-self-center">
                        <Show when=move || source.can_write.get()>
                            <input type="checkbox" class="h-5 w-5" aria-label=t_string!(i18n, cart_select_all_visible) prop:checked=move || all_visible_selected.get() disabled=move || visible_ids.with(|ids| ids.is_empty()) on:change=move |_| {
                                let ids = visible_ids.get_untracked();
                                if all_visible_selected.get_untracked() {
                                    selected_items.update(|s| { for id in &ids { s.remove(id); } });
                                } else {
                                    selected_items.update(|s| s.extend(ids));
                                }
                            } />
                        </Show>
                    </div>
                    {sort_header(SortKey::Name, item_label, false)}
                    {sort_header(SortKey::Acquired, qty_label, true)}
                    <div role="columnheader" class="uppercase tracking-wide">{t!(i18n, lists_workspace_quality)}</div>
                    {sort_header(SortKey::Price, cost_label, true)}
                    <div role="columnheader"></div>
                    <div role="columnheader"></div>
                </div>
                <ul class="text-sm" data-testid="cart-rows">
                    <For each=move || { visible.get().into_iter().map(|(item, _)| item).collect::<Vec<_>>() } key=|item| item.id children=move |initial| {
                        let id = initial.id;
                        let fallback = StoredValue::new(initial);
                        let item = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(item, _)| item.clone())).unwrap_or_else(|| fallback.get_value()));
                        let listings = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(_, listings)| listings.clone()).unwrap_or_default()));
                        let line = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(item, listings)| estimate::estimate_line(item, listings))));
                        view! { <CartRow item=item.into() listings=listings.into() line=line.into() selected_items expanded on_edit=source.edit on_delete=source.remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
                    } />
                </ul>
                <Show when=move || is_empty.get()>
                    <p class="px-4 py-6 text-center text-sm text-[color:var(--color-text-muted)]">{t!(i18n, cart_empty)}</p>
                </Show>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::cart::estimate::fixture_listing;

    fn item(id: i32, item_id: i32, quantity: i32, acquired: i32) -> ListItem {
        ListItem {
            id,
            item_id,
            list_id: 0,
            hq: None,
            quantity: Some(quantity),
            acquired: Some(acquired),
            target_price: None,
        }
    }

    fn ids(rows: &[(ListItem, Vec<ActiveListing>)]) -> Vec<i32> {
        rows.iter().map(|(item, _)| item.id).collect()
    }

    #[test]
    fn header_clicks_cycle_ascending_descending_off() {
        let first = next_sort(None, SortKey::Name);
        assert_eq!(
            first,
            Some(SortSpec {
                key: SortKey::Name,
                descending: false
            })
        );
        let second = next_sort(first, SortKey::Name);
        assert_eq!(
            second,
            Some(SortSpec {
                key: SortKey::Name,
                descending: true
            })
        );
        assert_eq!(next_sort(second, SortKey::Name), None);
        assert_eq!(
            next_sort(second, SortKey::Price),
            Some(SortSpec {
                key: SortKey::Price,
                descending: false
            }),
            "switching column starts ascending again"
        );
    }

    #[test]
    fn sort_by_estimate_puts_unknown_lines_last_and_is_stable_by_id() {
        let mut rows = vec![
            (item(3, 30, 2, 0), vec![]),
            (item(1, 10, 2, 0), vec![fixture_listing(1, 50, 5, false)]),
            (item(2, 20, 2, 0), vec![fixture_listing(2, 20, 5, false)]),
            (item(4, 40, 2, 0), vec![]),
        ];
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: false,
            },
            |_| None,
        );
        assert_eq!(ids(&rows), [2, 1, 3, 4]);
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: true,
            },
            |_| None,
        );
        assert_eq!(
            ids(&rows),
            [3, 4, 1, 2],
            "ties keep ascending row id in either direction"
        );
    }

    #[test]
    fn sort_by_name_and_remaining_quantity() {
        let names = |id: i32| match id {
            10 => Some("Maple Log"),
            20 => Some("Bronze Ingot"),
            _ => None,
        };
        let mut rows = vec![(item(1, 10, 5, 4), vec![]), (item(2, 20, 3, 0), vec![])];
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Name,
                descending: false,
            },
            names,
        );
        assert_eq!(ids(&rows), [2, 1]);
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Acquired,
                descending: false,
            },
            names,
        );
        assert_eq!(ids(&rows), [1, 2], "one unit left sorts before three");
    }

    #[test]
    fn mobile_sort_options_round_trip() {
        for value in [
            "name",
            "name-desc",
            "acquired",
            "acquired-desc",
            "price",
            "price-desc",
        ] {
            assert_eq!(sort_option(parse_sort_option(value)), value);
        }
        assert_eq!(parse_sort_option(""), None);
        assert_eq!(parse_sort_option("bogus"), None);
        assert_eq!(sort_option(None), "");
    }
}
