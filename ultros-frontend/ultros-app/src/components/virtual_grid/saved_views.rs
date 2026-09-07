//! Named analyzer views capture the complete query, including hidden filters,
//! column order and widths, and selected price inputs. Each grid owns its list.
use codee::string::JsonSerdeCodec;
use leptos::{html::Div, prelude::*};
use leptos_router::{location::Url, params::ParamsMap};
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

/// A built-in view the page offers above the user's own. The label arrives
/// already localized: `t!` needs a literal key, so a key passed in as data
/// could not be resolved here.
#[derive(Clone, Debug, PartialEq)]
pub struct GridPresetView {
    pub label: String,
    /// Query string including the leading `?`.
    pub query: String,
}

/// Query keys that say *where you are*, not *what you filtered*. They are
/// stripped when a view is saved and re-applied from the live URL when one is
/// opened, so a view stays portable across worlds and languages.
///
/// `world` matters because four analyzers (recipe, venture, leve, scrip) carry
/// the selected world in the query rather than the path. Without this, opening
/// a view there would drop the world and silently fall back to the home world,
/// and saving one would bake a world into a list the component documents as
/// world-independent.
const CONTEXT_KEYS: [&str; 2] = ["lang", "world"];

/// Every pair whose key is not context, in order.
///
/// `ParamsMap` is a multimap whose `insert` *appends*, so a stored view that
/// already carries a `world` cannot be corrected in place — the map has to be
/// rebuilt around the keys being replaced.
fn without_context(query: ParamsMap) -> ParamsMap {
    let mut kept = ParamsMap::new();
    for (key, value) in query {
        if !CONTEXT_KEYS.iter().any(|context| key == *context) {
            kept.insert(key, value);
        }
    }
    kept
}

/// The live query, minus the context keys. Uses `remove` rather than
/// [`without_context`] to keep the key order the stored views already have.
fn saved_query(mut query: ParamsMap) -> String {
    for key in CONTEXT_KEYS {
        query.remove(key);
    }
    query.to_query_string()
}

fn parse_query(query: &str) -> ParamsMap {
    let mut map = ParamsMap::new();
    for pair in query
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty())
    {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(Url::unescape(key), Url::unescape(value));
    }
    map
}

/// Apply a view to the current analyzer destination, carrying the live context
/// params over.
///
/// The live value *overwrites* rather than appends: a view stored before
/// [`CONTEXT_KEYS`] existed may still carry its own `world`/`lang`, and
/// appending would emit the key twice.
fn view_href(pathname: &str, query: &str, context: &ParamsMap) -> String {
    let mut params = without_context(parse_query(query));
    for key in CONTEXT_KEYS {
        if let Some(value) = context.get(key) {
            params.insert(key, value);
        }
    }
    format!("{pathname}{}", params.to_query_string())
}

/// The popover's list body: built-ins first, then the reader's own.
///
/// Split out of [`GridSavedViews`] so a render test can reach it without the
/// `open` gate hiding everything, the same reason `ColumnsPickerList` is a
/// sibling of `ControlBar` (`components/control_bar.rs:154`).
#[component]
fn GridSavedViewsList(
    #[prop(into)] presets: Signal<Vec<GridPresetView>>,
    #[prop(into)] views: Signal<Vec<GridSavedView>>,
    set_views: WriteSignal<Vec<GridSavedView>>,
    /// Closed when an entry is picked. Load-bearing: a view only changes the
    /// query, and `use_dismissable` tracks the pathname alone
    /// (`components/dismissable.rs:50-53`), so nothing else would close it.
    open: RwSignal<bool>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    view! {
        <div class="max-h-64 overflow-y-auto flex flex-col gap-1">
            // A page that ships presets is never empty.
            <Show when=move || views.with(Vec::is_empty) && presets.with(Vec::is_empty)>
                <p class="text-[color:var(--color-text-muted)]">
                    {t!(i18n, grid_saved_views_empty)}
                </p>
            </Show>
            {move || {
                presets.get().into_iter().map(|preset| {
                    let href = view_href(
                        &location.pathname.get(),
                        &preset.query,
                        &location.query.get(),
                    );
                    view! {
                        <a
                            class="btn-ghost w-full min-w-0 justify-start break-words"
                            href=href
                            on:click=move |_| open.set(false)
                        >
                            {preset.label}
                        </a>
                    }
                }).collect_view()
            }}
            // Scrolls with the list, unlike the save form's rule below it.
            {move || (!presets.with(Vec::is_empty) && !views.with(Vec::is_empty))
                .then(|| view! {
                    <div class="my-1 border-t border-[color:var(--color-outline)]"></div>
                })}
            {move || {
                views.get().into_iter().enumerate().map(|(index, saved)| {
                    let href = view_href(
                        &location.pathname.get(),
                        &saved.query,
                        &location.query.get(),
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
    }
}

/// Use a stable analyzer id so changing worlds does not change the saved list.
#[component]
pub fn GridSavedViews(
    #[prop(into)] id: String,
    /// Views the page ships with, rendered above the reader's own and without
    /// a delete button. Five of the six analyzers omit it.
    ///
    /// A `Signal` rather than a plain `Vec` because the labels are localized
    /// and the locale changes *in place* — `LanguagePicker` calls
    /// `i18n.set_locale` with no navigation — so a `Vec` built once would keep
    /// the language it was built in.
    #[prop(optional, into)]
    presets: Signal<Vec<GridPresetView>>,
) -> impl IntoView {
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
                class="sticky-bar-button sticky-bar-button-shrink"
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
                <span class="hidden md:inline sticky-bar-button-label">{t!(i18n, analyzer_saved_views)}</span>
            </button>
            <Show when=move || open.get()>
                <div class="sticky-bar-popover p-3 w-[min(92vw,20rem)] flex flex-col gap-2 text-sm">
                    <GridSavedViewsList presets=presets views=views set_views=set_views open=open />
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
    use leptos_i18n::context::init_i18n_context;

    fn preset(label: &str, query: &str) -> GridPresetView {
        GridPresetView {
            label: label.into(),
            query: query.into(),
        }
    }

    fn saved(name: &str, query: &str) -> GridSavedView {
        GridSavedView {
            name: name.into(),
            query: query.into(),
        }
    }

    /// The list body only — the popover's `open` gate would otherwise hide
    /// every assertion below.
    fn render_list(presets: Vec<GridPresetView>, views: Vec<GridSavedView>) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let (views, set_views) = signal(views);
            view! {
                <GridSavedViewsList
                    presets=Signal::derive(move || presets.clone())
                    views=views
                    set_views=set_views
                    open=RwSignal::new(true)
                />
            }
            .to_html()
        })
    }

    /// A built-in is a link and nothing else — the delete button is what marks
    /// a view as the reader's own.
    #[test]
    fn built_in_views_render_without_a_delete_button() {
        let html = render_list(vec![preset("50K+ profit", "?profit=50000")], vec![]);
        assert_eq!(html.matches("<a").count(), 1, "{html}");
        assert_eq!(html.matches("<button").count(), 0, "{html}");
        assert!(html.contains("50K+ profit"), "{html}");
    }

    #[test]
    fn user_views_keep_their_delete_button_and_sit_below_a_divider() {
        let html = render_list(
            vec![preset("50K+ profit", "?profit=50000")],
            vec![saved("Mine", "?roi=30")],
        );
        assert_eq!(html.matches("<button").count(), 1, "{html}");
        let divider = html.find("border-t").expect("divider between sections");
        let built_in = html.find("50K+ profit").expect("preset label");
        let mine = html.find("Mine").expect("user view name");
        assert!(built_in < divider && divider < mine, "{html}");
    }

    /// The divider is a separator, so it needs something on both sides.
    #[test]
    fn one_section_alone_renders_no_divider() {
        assert!(!render_list(vec![preset("P", "?a=1")], vec![]).contains("border-t"));
        assert!(!render_list(vec![], vec![saved("M", "?a=1")]).contains("border-t"));
    }

    #[test]
    fn the_empty_state_shows_only_when_there_is_nothing_at_all() {
        let empty = render_list(vec![], vec![]);
        let marker = "text-[color:var(--color-text-muted)]";
        assert!(empty.contains(marker), "{empty}");
        // A page that ships presets is never empty, even before the reader
        // saves anything of their own.
        assert!(!render_list(vec![preset("P", "?a=1")], vec![]).contains(marker));
        assert!(!render_list(vec![], vec![saved("M", "?a=1")]).contains(marker));
    }

    fn context(pairs: &[(&'static str, &'static str)]) -> ParamsMap {
        ParamsMap::from_iter(pairs.iter().copied())
    }

    #[test]
    fn saved_views_capture_grid_and_pricing_state_without_context() {
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

    /// The world is *where you are*, not a filter. Four analyzers carry it in
    /// the query, so leaving it in would pin a saved view to one world.
    #[test]
    fn saving_a_view_drops_the_selected_world() {
        let saved = saved_query(context(&[
            ("world", "Gilgamesh"),
            ("lang", "ja"),
            ("profit", "1000"),
        ]));
        assert_eq!(saved, "?profit=1000");
    }

    #[test]
    fn loading_a_view_keeps_destination_language_and_world() {
        assert_eq!(
            view_href(
                "/venture/Gilgamesh",
                "?price=median",
                &context(&[("lang", "de")])
            ),
            "/venture/Gilgamesh?price=median&lang=de",
        );
        assert_eq!(
            view_href("/leve/Sargatanas", "", &ParamsMap::new()),
            "/leve/Sargatanas",
        );
        assert_eq!(
            view_href("/leve/Sargatanas", "", &context(&[("lang", "ja")])),
            "/leve/Sargatanas?lang=ja",
        );
        // The query-param analyzers: without this the preset would drop the
        // world and silently fall back to the reader's home world.
        assert_eq!(
            view_href(
                "/venture-analyzer",
                "?profit=50000",
                &context(&[("world", "Gilgamesh")]),
            ),
            "/venture-analyzer?profit=50000&world=Gilgamesh",
        );
    }

    /// Views stored before `CONTEXT_KEYS` existed still carry their own
    /// `world`; the live one must overwrite it, not sit beside it.
    #[test]
    fn a_stale_view_yields_exactly_one_world() {
        let href = view_href(
            "/venture-analyzer",
            "?world=Sargatanas&profit=50000",
            &context(&[("world", "Gilgamesh")]),
        );
        assert_eq!(href.matches("world=").count(), 1, "{href}");
        assert!(href.contains("world=Gilgamesh"), "{href}");
    }

    /// A trailing `&` in a hand-written preset query must not survive into an
    /// href as an empty pair.
    #[test]
    fn a_trailing_separator_is_dropped() {
        assert_eq!(
            view_href("/vendor-resale/Gilgamesh", "?roi=100&", &ParamsMap::new()),
            "/vendor-resale/Gilgamesh?roi=100",
        );
    }
}
