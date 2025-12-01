use crate::{
    components::{search_result::*, virtual_scroller::*},
    global_state::home_world::get_price_zone,
};
use gloo_timers::future::TimeoutFuture;
use icondata as i;
use leptos::{html::Input, prelude::*, task::spawn_local};
use leptos_hotkeys::use_hotkeys;
use leptos_icons::*;
use leptos_router::{NavigateOptions, hooks::use_navigate};
use std::sync::Arc;
use web_sys::KeyboardEvent;
use ultros_api_types::search::SearchResult;
use sublime_fuzzy::{FuzzySearch, Match, Scoring};

pub(crate) fn fuzzy_search(query: &str, target: &str) -> Option<Match> {
    let scoring = Scoring::default();
    let search = FuzzySearch::new(query, target)
        .case_insensitive()
        .score_with(&scoring);
    search.best_match()
}

#[component]
pub fn SearchBox() -> impl IntoView {
    let text_input = NodeRef::<Input>::new();
    let (search, set_search) = signal(String::new());
    let navigate = use_navigate();
    let (active, set_active) = signal(false);

use crate::api::search as api_search;

    // Use a signal for results to avoid hydration issues with Resources
    let (search_results, set_search_results) = signal(Vec::new());

    Effect::new(move |_| {
        let s = search.get();
        spawn_local(async move {
            if s.trim().is_empty() {
                set_search_results.set(vec![]);
                return;
            }
            match api_search(&s).await {
                Ok(results) => {
                    set_search_results.set(results);
                }
                Err(e) => {
                    log::error!("Search failed: {}", e);
                    set_search_results.set(vec![]);
                }
            }
        });
    });

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

    let item_search = move || search_results.get();

    let navigate_keydown = navigate.clone();
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
             if let Some(first) = item_search().first() {
                navigate_keydown(&first.url, NavigateOptions::default());
                set_search("".to_string());
                if let Some(input) = text_input.get() {
                    let _ = input.blur();
                }
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
                    placeholder="Search items, currencies, categories... (⌘K / CTRL K)"
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
                        key=move |result: &SearchResult| result.url.clone()
                        view=move |result: SearchResult| {
                            let url = result.url.clone();
                            let navigate = navigate.clone();
                            view! {
                                <div class="p-2 hover:bg-[color:var(--color-background-elevated)] cursor-pointer flex items-center gap-2"
                                     on:click=move |_| {
                                         navigate(&url, NavigateOptions::default());
                                         set_search("".to_string());
                                     }
                                >
                                    // Icon based on type or icon_id
                                    {
                                        if let Some(icon_id) = result.icon_id {
                                            if icon_id > 0 {
                                                view! {
                                                    <div class="w-8 h-8 flex-shrink-0">
                                                        <img
                                                            src=format!("/static/itemicon/{}?size=Small", icon_id)
                                                            class="w-full h-full object-contain"
                                                            loading="lazy"
                                                        />
                                                    </div>
                                                }.into_any()
                                            } else {
                                                match result.result_type.as_str() {
                                                    "item" => view! { <Icon icon=i::FaBoxOpenSolid /> }.into_any(),
                                                    "currency" => view! { <Icon icon=i::FaCoinsSolid /> }.into_any(),
                                                    "category" => view! { <Icon icon=i::FaListSolid /> }.into_any(),
                                                    "job equipment" => view! { <Icon icon=i::FaUserSolid /> }.into_any(),
                                                    _ => view! { <Icon icon=i::AiSearchOutlined /> }.into_any(),
                                                }
                                            }
                                        } else {
                                            match result.result_type.as_str() {
                                                "item" => view! { <Icon icon=i::FaBoxOpenSolid /> }.into_any(),
                                                "currency" => view! { <Icon icon=i::FaCoinsSolid /> }.into_any(),
                                                "category" => view! { <Icon icon=i::FaListSolid /> }.into_any(),
                                                "job equipment" => view! { <Icon icon=i::FaUserSolid /> }.into_any(),
                                                _ => view! { <Icon icon=i::AiSearchOutlined /> }.into_any(),
                                            }
                                        }
                                    }
                                    <div class="flex flex-col">
                                        <span class="font-medium">{result.title}</span>
                                        <span class="text-xs text-[color:var(--color-text-muted)]">{result.result_type}</span>
                                    </div>
                                </div>
                            }
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
