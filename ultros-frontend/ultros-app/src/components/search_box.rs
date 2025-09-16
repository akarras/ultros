use crate::{
    components::{search_result::*, virtual_scroller::*},
    global_state::home_world::get_price_zone,
};
use gloo_timers::future::TimeoutFuture;
use icondata as i;
use leptos::{html::Input, prelude::*, task::spawn_local};
use leptos_hotkeys::use_hotkeys;
// use leptos_hotkeys::use_hotkeys;
use leptos_icons::*;
use leptos_router::{hooks::use_navigate, NavigateOptions};
use std::cmp::Reverse;
use sublime_fuzzy::{FuzzySearch, Match, Scoring};
use web_sys::KeyboardEvent;

pub(crate) fn fuzzy_search(query: &str, target: &str) -> Option<Match> {
    let scoring = Scoring::default();
    let search = FuzzySearch::new(query, target)
        .case_insensitive()
        .score_with(&scoring);
    search.best_match()
}

/// SearchBox primarily searches through item names- there might be better ways to filter the views down the line.
#[component]
pub fn SearchBox() -> impl IntoView {
    let text_input = NodeRef::<Input>::new();
    let (search, set_search) = signal(String::new());
    let navigate = use_navigate();
    let (active, set_active) = signal(false);
    use_hotkeys!(("MetaLeft+KeyK,ControlLeft+KeyK", "*") => move |_| {
        set_active(true);
        if let Some(input) = text_input.get() {
            let _ = input.focus();
        }
    });

    leptos_hotkeys::use_hotkeys_ref(
        text_input,
        "Escape".to_string(),
        Callback::new(move |_| {}),
        vec!["*".to_string()],
    );
    let on_input = move |ev| {
        set_search(event_target_value(&ev));
    };
    let focus_in = move |_| set_active(true);
    let focus_out = move |_| {
        spawn_local(async move {
            TimeoutFuture::new(250).await;
            set_active(false);
        })
    };
    let items = &xiv_gen_db::data().items;
    let item_search = move || {
        search.with(|s| {
            if s.is_empty() {
                return vec![];
            }
            let q = s.trim();
            if q.is_empty() {
                return vec![];
            }
            let ql = q.to_lowercase();

            // very short queries: cheap substring match, limit output
            if ql.len() < 2 {
                let mut results = items
                    .iter()
                    .filter(|(_, i)| i.item_search_category.0 > 0)
                    .filter(|(_, i)| i.name.to_lowercase().contains(&ql))
                    .map(|(id, item)| (id, item))
                    .collect::<Vec<_>>();
                results.sort_by_key(|(_, i)| {
                    (Reverse(i.level_item.0), i.name.as_str().to_lowercase())
                });
                results.truncate(100);
                return results;
            }

            // prefilter candidates by substring to reduce fuzzy workload
            let candidates = items
                .iter()
                .filter(|(_, i)| i.item_search_category.0 > 0)
                .filter(|(_, i)| {
                    let name = i.name.as_str().to_lowercase();
                    if ql.len() < 3 {
                        true
                    } else {
                        name.contains(&ql)
                    }
                });

            let mut scored = candidates
                .flat_map(|(id, i)| fuzzy_search(q, &i.name).map(|m| (id, i, m)))
                .collect::<Vec<_>>();

            scored.sort_by_key(|(_, i, m)| (Reverse(m.score()), Reverse(i.level_item.0)));
            scored
                .into_iter()
                .map(|(id, item, _)| (id, item))
                .take(100)
                .collect::<Vec<_>>()
        })
    };
    let keydown = move |e: KeyboardEvent| {
        let key = e.key();
        if key == "Escape" {
            if search.get_untracked().is_empty() {
                if let Some(input) = text_input.get() {
                    let _ = input.blur();
                }
                set_active(false);
            } else {
                set_search("".to_string());
            }
        } else if key == "Enter" {
            if let Some((id, _)) = item_search().first() {
                let (zone, _) = get_price_zone();
                let id = id.0;
                let zone = zone.get_untracked();
                let price_zone = zone
                    .as_ref()
                    .map(|z| z.get_name())
                    .unwrap_or("North-America");

                navigate(
                    &format!("/item/{price_zone}/{id}"),
                    NavigateOptions::default(),
                );
                set_search("".to_string());
                text_input.get().unwrap().blur().unwrap();
            }
        }
    };
    view! {
        <div class="relative md:w-full sm:w-[424px]">
            <div class="relative">
                <input
                    node_ref=text_input
                    on:keydown=keydown
                    on:input=on_input
                    on:focusin=focus_in
                    on:focusout=focus_out
                    placeholder="Search items... (⌘K / CTRL K)"
                    class="input w-full pl-10"

                    type="text"
                    prop:value=search
                />
                <div class="absolute left-3 top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]">
                    <Icon icon=i::AiSearchOutlined />
                </div>
            </div>

            // Search Results
            <div
                class="absolute w-full mt-2 z-50 content-visible contain-content forced-layer"
                class:hidden=move || !active() || search().is_empty()
            >
                <div class="scroll-panel content-auto contain-layout contain-paint will-change-scroll forced-layer cis-42">
                    <VirtualScroller
                        each=Signal::derive(item_search)
                        key=move |(id, _item): &(&xiv_gen::ItemId, &xiv_gen::Item)| id.0
                        view=move |(id, _): (&xiv_gen::ItemId, &xiv_gen::Item)| {
                            let item_id = id.0;
                            view! { <ItemSearchResult item_id set_search search /> }
                        }
                        viewport_height=528.0
                        row_height=60.0
                        overscan=10
                        header_height=0.0
                        variable_height=false

                    />
                </div>
            </div>
        </div>
    }
    .into_any()
}
