//! Saved filter views, persisted to localStorage.
//!
//! Every Flip Finder filter is already a `query_signal`, so the complete
//! filter state *is* the URL query string. A saved view is therefore just
//! a name plus that string — no schema to migrate, and filters added later
//! are captured automatically because the payload is opaque.

use codee::string::JsonSerdeCodec;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};
use serde::{Deserialize, Serialize};

use crate::components::icon::Icon;
use crate::i18n::*;

pub const SAVED_VIEWS_KEY: &str = "ultros.flipfinder.views";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    /// Query string including the leading `?`, or empty for no filters.
    pub query: String,
    /// `Some(world)` pins the view to that world, making it a destination
    /// that navigates. `None` applies the filters to whatever world is
    /// open. Unpinned is the default; pinning is opt-in at save time and
    /// exists for players with characters on different worlds.
    pub world: Option<String>,
}

/// Resolve a view to an href. Pinned views navigate worlds; unpinned views
/// stay put.
pub fn view_href(view: &SavedView, current_world: &str) -> String {
    let world = view.world.as_deref().unwrap_or(current_world);
    format!("/flip-finder/{world}{}", view.query)
}

/// The former preset buttons, now the built-in entries of the same menu.
/// All six require a sale within 24 hours: a sale a week old is weak
/// evidence anyone is buying the item today, which is the question a flip
/// turns on. Verified against live Gilgamesh data — no view collapses
/// under the tighter window, the thinnest being "500% return" at 9 rows
/// down from 60.
///
/// `name` is an i18n key, not a display string — the menu resolves it at
/// render time (see `SavedViewsMenu`'s `built_in_label`). User-saved views
/// store a literal name instead; the menu tells the two apart by checking
/// membership in this list.
pub fn built_in_views() -> Vec<SavedView> {
    [
        (
            "analyzer_preset_realistic",
            "?min-buy=5000&last-sold=1d&roi=30&sort=profit-per-day",
        ),
        (
            "analyzer_preset_big_ticket",
            "?min-buy=100000&last-sold=1d&roi=20&sort=profit",
        ),
        (
            "analyzer_preset_volume",
            "?min-buy=1000&last-sold=1d&sort=profit-per-day",
        ),
        (
            "analyzer_preset_300_return",
            "?min-buy=1000&last-sold=1d&roi=300&profit=0&sort=profit",
        ),
        (
            "analyzer_preset_500_return",
            "?min-buy=10000&last-sold=1d&roi=500&profit=200000",
        ),
        (
            "analyzer_preset_100k_profit",
            "?min-buy=1000&last-sold=1d&profit=100000",
        ),
    ]
    .into_iter()
    .map(|(name, query)| SavedView {
        name: name.to_string(),
        query: query.to_string(),
        world: None,
    })
    .collect()
}

/// Views menu mounted in the sticky bar's first row. Combines two
/// affordances behind one component so the Flip Finder toolbar stays a
/// single mount point:
///
/// - **Views**: lists the six built-ins plus anything saved, each an `<a>`
///   that navigates to `view_href`. Saved (not built-in) entries also get a
///   delete button.
/// - **Save view**: names the *current* URL query string and appends it to
///   the saved list, optionally pinned to `current_world`.
#[component]
pub fn SavedViewsMenu(#[prop(into)] current_world: Signal<String>) -> impl IntoView {
    let i18n = use_i18n();
    let query = use_query_map();
    let (views, set_views, _) = use_local_storage_with_options::<Vec<SavedView>, JsonSerdeCodec>(
        SAVED_VIEWS_KEY,
        // Private-browsing / storage-disabled must degrade to session-only,
        // never panic. `delay_during_hydration` is load-bearing for the same
        // reason it is in `RecentItems::new` (recently_viewed.rs): reading
        // localStorage synchronously during component setup races hydration
        // and can produce a CSR/SSR shape mismatch (GlitchTip 3147 + 4327).
        UseStorageOptions::default().delay_during_hydration(true),
    );

    let list_open = RwSignal::new(false);
    let save_open = RwSignal::new(false);
    let new_name = RwSignal::new(String::new());
    let pin_to_world = RwSignal::new(false);

    // Built-in `name`s are i18n keys (see `built_in_views`); resolve them the
    // same way `col_label` resolves column ids in analyzer.rs. Any name that
    // isn't a recognized built-in key (i.e. a user-saved view's literal
    // name) passes through unchanged.
    let built_in_label = move |key: &str| -> String {
        match key {
            "analyzer_preset_realistic" => t_string!(i18n, analyzer_preset_realistic).to_string(),
            "analyzer_preset_big_ticket" => t_string!(i18n, analyzer_preset_big_ticket).to_string(),
            "analyzer_preset_volume" => t_string!(i18n, analyzer_preset_volume).to_string(),
            "analyzer_preset_300_return" => t_string!(i18n, analyzer_preset_300_return).to_string(),
            "analyzer_preset_500_return" => t_string!(i18n, analyzer_preset_500_return).to_string(),
            "analyzer_preset_100k_profit" => {
                t_string!(i18n, analyzer_preset_100k_profit).to_string()
            }
            other => other.to_string(),
        }
    };

    let save_current_view = move |_| {
        let name = new_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        let query_string = query.get_untracked().to_query_string();
        let world = pin_to_world
            .get_untracked()
            .then(|| current_world.get_untracked());
        set_views.update(|vs| {
            vs.push(SavedView {
                name,
                query: query_string,
                world,
            })
        });
        new_name.set(String::new());
        pin_to_world.set(false);
        save_open.set(false);
    };

    view! {
        <div class="relative flex items-center gap-2">
            <button
                class="sticky-bar-button"
                aria-expanded=move || list_open.get().to_string()
                on:click=move |_| {
                    save_open.set(false);
                    list_open.update(|v| *v = !*v);
                }
            >
                {t!(i18n, analyzer_saved_views)}
            </button>
            <button
                class="sticky-bar-button"
                aria-expanded=move || save_open.get().to_string()
                on:click=move |_| {
                    list_open.set(false);
                    save_open.update(|v| *v = !*v);
                }
            >
                {t!(i18n, analyzer_save_view)}
            </button>

            <Show when=move || list_open.get()>
                <div class="sticky-bar-popover p-2 w-[min(92vw,16rem)] flex flex-col gap-1 text-sm">
                    {move || {
                        built_in_views()
                            .into_iter()
                            .map(|v| {
                                let href = view_href(&v, &current_world.get());
                                let label = built_in_label(&v.name);
                                view! { <a class="btn-ghost justify-start" href=href>{label}</a> }
                            })
                            .collect_view()
                    }}
                    {move || {
                        (!views.get().is_empty())
                            .then(|| {
                                view! {
                                    <div class="my-1 border-t border-[color:var(--color-outline)]" />
                                }
                            })
                    }}
                    {move || {
                        views
                            .get()
                            .into_iter()
                            .enumerate()
                            .map(|(i, v)| {
                                let href = view_href(&v, &current_world.get());
                                let name = v.name.clone();
                                view! {
                                    <div class="flex items-center gap-1">
                                        <a class="btn-ghost flex-1 justify-start" href=href>
                                            {name}
                                        </a>
                                        <button
                                            class="sticky-bar-button"
                                            aria-label=t_string!(i18n, analyzer_delete_view)
                                            on:click=move |_| {
                                                set_views
                                                    .update(|vs| {
                                                        if i < vs.len() {
                                                            vs.remove(i);
                                                        }
                                                    });
                                            }
                                        >
                                            <Icon icon=icondata::MdiClose />
                                        </button>
                                    </div>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </Show>

            <Show when=move || save_open.get()>
                <div class="sticky-bar-popover p-3 w-[min(92vw,18rem)] flex flex-col gap-2 text-sm">
                    <input
                        class="input input-sm"
                        type="text"
                        placeholder=t_string!(i18n, analyzer_view_name_placeholder)
                        prop:value=move || new_name.get()
                        on:input=move |ev| new_name.set(event_target_value(&ev))
                    />
                    <label class="inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]">
                        <input
                            type="checkbox"
                            class="accent-brand-300"
                            prop:checked=move || pin_to_world.get()
                            on:change=move |ev| pin_to_world.set(event_target_checked(&ev))
                        />
                        <span>{t!(i18n, analyzer_pin_view_to_world)}</span>
                    </label>
                    <button class="btn-secondary" on:click=save_current_view>
                        {t!(i18n, analyzer_save_view)}
                    </button>
                </div>
            </Show>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpinned_view_keeps_the_current_world() {
        let v = SavedView {
            name: "Slot savers".into(),
            query: "?sort=profit-per-day&vel=0.2".into(),
            world: None,
        };
        assert_eq!(
            view_href(&v, "Sargatanas"),
            "/flip-finder/Sargatanas?sort=profit-per-day&vel=0.2"
        );
    }

    #[test]
    fn pinned_view_navigates_to_its_own_world() {
        let v = SavedView {
            name: "Gilg dashboard".into(),
            query: "?sort=profit".into(),
            world: Some("Gilgamesh".into()),
        };
        assert_eq!(
            view_href(&v, "Sargatanas"),
            "/flip-finder/Gilgamesh?sort=profit"
        );
    }

    #[test]
    fn empty_query_produces_a_clean_path() {
        let v = SavedView {
            name: "All".into(),
            query: String::new(),
            world: None,
        };
        assert_eq!(view_href(&v, "Gilgamesh"), "/flip-finder/Gilgamesh");
    }

    #[test]
    fn saved_view_round_trips_through_json() {
        let v = SavedView {
            name: "Slot savers".into(),
            query: "?vel=0.2".into(),
            world: Some("Gilgamesh".into()),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<SavedView>(&json).unwrap(), v);
    }

    #[test]
    fn every_built_in_view_requires_a_sale_within_one_day() {
        // A sale a week old is weak evidence anyone is buying the item
        // today, which is the question a flip turns on.
        for v in built_in_views() {
            assert!(
                v.query.contains("last-sold=1d"),
                "built-in view {:?} must use last-sold=1d, got {:?}",
                v.name,
                v.query
            );
        }
    }

    #[test]
    fn built_in_views_are_unpinned() {
        for v in built_in_views() {
            assert_eq!(v.world, None, "built-in {:?} must not pin a world", v.name);
        }
    }
}
