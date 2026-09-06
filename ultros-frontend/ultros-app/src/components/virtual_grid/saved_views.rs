//! Named analyzer views capture the complete query, including hidden filters,
//! column order and widths, and selected price inputs. Each grid owns its list.
use codee::string::JsonSerdeCodec;
use leptos::{html::Div, prelude::*};
use leptos_router::params::ParamsMap;
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};
use serde::{Deserialize, Serialize};

use crate::components::{
    app_link::use_location_or_default, dismissable::use_dismissable, icon::Icon,
};
use crate::i18n::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct GridSavedView {
    name: String,
    query: String,
}

fn saved_query(mut query: ParamsMap) -> String {
    query.remove("lang");
    query.to_query_string()
}

/// Apply a view to the current analyzer destination and keep the current locale.
fn view_href(pathname: &str, query: &str, language: Option<String>) -> String {
    let mut href = format!("{pathname}{query}");
    if let Some(language) = language {
        let mut locale = ParamsMap::new();
        locale.insert("lang", language);
        href.push(if query.is_empty() { '?' } else { '&' });
        href.push_str(locale.to_query_string().trim_start_matches('?'));
    }
    href
}

/// Use a stable analyzer id so changing worlds does not change the saved list.
#[component]
pub fn GridSavedViews(#[prop(into)] id: String) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let (views, set_views, _) = use_local_storage_with_options::<Vec<GridSavedView>, JsonSerdeCodec>(
        format!("ultros.grid.{id}.views"),
        // Keep the initial render identical on SSR and the client. If
        // storage is disabled, the signal still supports session use.
        UseStorageOptions::default().delay_during_hydration(true),
    );
    let open = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let container = NodeRef::<Div>::new();
    let popover = use_dismissable(container, move || open.set(false));

    view! {
        <div class="relative" node_ref=container data-grid-saved-views>
            <button
                type="button"
                class="sticky-bar-button"
                aria-label=t_string!(i18n, analyzer_saved_views)
                aria-expanded=move || open.get().to_string()
                on:click=move |_| {
                    let opening = !open.get_untracked();
                    if opening {
                        popover.opening();
                    }
                    open.set(opening);
                }
            >
                <Icon icon=icondata::MdiBookmarkMultipleOutline />
                <span>{t!(i18n, analyzer_saved_views)}</span>
            </button>
            <Show when=move || open.get()>
                <div class="sticky-bar-popover p-3 w-[min(92vw,20rem)] flex flex-col gap-2 text-sm">
                    <div class="max-h-64 overflow-y-auto flex flex-col gap-1">
                        <Show when=move || views.with(Vec::is_empty)>
                            <p class="text-[color:var(--color-text-muted)]">
                                {t!(i18n, grid_saved_views_empty)}
                            </p>
                        </Show>
                        {move || {
                            views.get().into_iter().enumerate().map(|(index, saved)| {
                                let href = view_href(
                                    &location.pathname.get(),
                                    &saved.query,
                                    location.query.with(|query| query.get("lang")),
                                );
                                let delete_label = format!(
                                    "{}: {}",
                                    t_string!(i18n, analyzer_delete_view),
                                    saved.name,
                                );
                                view! {
                                    <div class="flex items-center gap-1">
                                        <a
                                            class="btn-ghost flex-1 min-w-0 justify-start break-words"
                                            href=href
                                            on:click=move |_| open.set(false)
                                        >
                                            {saved.name}
                                        </a>
                                        <button
                                            type="button"
                                            class="sticky-bar-button shrink-0"
                                            aria-label=delete_label
                                            on:click=move |_| set_views.update(|views| {
                                                if index < views.len() {
                                                    views.remove(index);
                                                }
                                            })
                                        >
                                            <Icon icon=icondata::MdiClose />
                                        </button>
                                    </div>
                                }
                            }).collect_view()
                        }}
                    </div>
                    <form
                        class="flex flex-col gap-2 border-t border-[color:var(--color-outline)] pt-2"
                        on:submit=move |event| {
                            event.prevent_default();
                            let entered = name.get_untracked().trim().to_string();
                            if entered.is_empty() {
                                return;
                            }
                            let query = saved_query(location.query.get_untracked());
                            set_views.update(|views| views.push(GridSavedView {
                                name: entered,
                                query,
                            }));
                            name.set(String::new());
                        }
                    >
                        <label class="flex flex-col gap-1">
                            <span>{t!(i18n, grid_view_name)}</span>
                            <input
                                type="text"
                                class="input input-sm"
                                maxlength="100"
                                required
                                prop:value=move || name.get()
                                on:input=move |event| name.set(event_target_value(&event))
                            />
                        </label>
                        <button
                            type="submit"
                            class="btn-secondary"
                            disabled=move || name.with(|name| name.trim().is_empty())
                        >
                            {t!(i18n, analyzer_save_view)}
                        </button>
                    </form>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_views_capture_grid_and_pricing_state_without_language() {
        let query = ParamsMap::from_iter([
            ("lang", "ja"),
            ("gf", "{\"median\":{\"op\":\"gte\",\"value\":\"100\"}}"),
            ("l", "median:180,profit:120"),
            ("price", "median"),
            ("sort", "grid:median"),
            ("cols", "median,trend"),
        ]);
        let serialized = saved_query(query.clone());
        let mut expected = query;
        expected.remove("lang");
        assert_eq!(serialized, expected.to_query_string());
        let saved = GridSavedView {
            name: "Median".into(),
            query: serialized,
        };
        assert_eq!(
            serde_json::from_str::<GridSavedView>(&serde_json::to_string(&saved).unwrap()).unwrap(),
            saved,
        );
    }

    #[test]
    fn loading_a_view_keeps_destination_and_current_language() {
        assert_eq!(
            view_href("/venture/Gilgamesh", "?price=median", Some("de".into())),
            "/venture/Gilgamesh?price=median&lang=de",
        );
        assert_eq!(view_href("/leve/Sargatanas", "", None), "/leve/Sargatanas");
        assert_eq!(
            view_href("/leve/Sargatanas", "", Some("ja".into())),
            "/leve/Sargatanas?lang=ja",
        );
    }
}
