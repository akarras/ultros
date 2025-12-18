use crate::components::icon::Icon;
use crate::components::loading::Loading;
use crate::components::virtual_scroller::*;
use gloo_timers::future::TimeoutFuture;
use icondata as i;
use leptos::{html::Input, prelude::*, task::spawn_local};
use leptos_hotkeys::use_hotkeys;
use leptos_router::{NavigateOptions, hooks::use_navigate};
use ultros_api_types::search::SearchResult;
use web_sys::KeyboardEvent;

#[component]
pub fn SearchBox() -> impl IntoView {
    let text_input = NodeRef::<Input>::new();
    let (search, set_search) = signal(String::new());
    let navigate = use_navigate();
    let (active, set_active) = signal(false);
    let (loading, set_loading) = signal(false);

    use crate::api::search as api_search;

    // Search results and request tracking
    let (search_results, set_search_results) = signal::<Vec<SearchResult>>(Vec::new());
    let (search_id, set_search_id) = signal(0usize);

    // Keyboard navigation focus handling
    let (focused_index, set_focused_index) = signal::<Option<usize>>(None);

    // Currently-focused result's URL for highlight/selection
    let focused_url: Signal<Option<String>> = Signal::derive(move || {
        focused_index.get().and_then(|idx| {
            search_results
                .with(|v: &Vec<SearchResult>| v.get(idx).map(|r: &SearchResult| r.url.clone()))
        })
    });

    // When results change, reset the focused index to the first item (if any)
    Effect::new(move |_| {
        let len = search_results.with(|v: &Vec<SearchResult>| v.len());
        if len > 0 {
            set_focused_index.set(Some(0));
        } else {
            set_focused_index.set(None);
        }
    });

    // Debounced search effect with cancellation via serial search_id
    Effect::new(move |_| {
        let s = search.get();
        set_search_id.update(|n| *n += 1);
        let current_id = search_id.get_untracked();

        spawn_local(async move {
            TimeoutFuture::new(300).await;

            if search_id.get_untracked() != current_id {
                return;
            }

            if s.trim().is_empty() {
                set_search_results.set(vec![]);
                return;
            }

            set_loading.set(true);
            match api_search(&s).await {
                Ok(results) => {
                    if search_id.get_untracked() == current_id {
                        set_search_results.set(results);
                        set_loading.set(false);
                    }
                }
                Err(e) => {
                    if search_id.get_untracked() == current_id {
                        log::error!("Search failed: {}", e);
                        set_search_results.set(vec![]);
                        set_loading.set(false);
                    }
                }
            }
        });
    });

    // Hotkey to focus search (Cmd+K / Ctrl+K)
    use_hotkeys!(("MetaLeft+KeyK,ControlLeft+KeyK", "*") => move |_| {
        set_active(true);
        if let Some(input) = text_input.get() {
            let _ = input.focus();
        }
    });

    // Escape binding on the input (kept as-is)
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

    // Keyboard navigation for Up/Down; Enter uses focused item
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
        } else if key == "ArrowDown" {
            e.prevent_default();
            let len = search_results.with(|v: &Vec<SearchResult>| v.len());
            if len > 0 {
                let next = focused_index
                    .get_untracked()
                    .unwrap_or(0)
                    .saturating_add(1)
                    .min(len.saturating_sub(1));
                set_focused_index.set(Some(next));
            }
        } else if key == "ArrowUp" {
            e.prevent_default();
            let len = search_results.with(|v: &Vec<SearchResult>| v.len());
            if len > 0 {
                let current = focused_index.get_untracked().unwrap_or(0);
                let next = current.saturating_sub(1);
                set_focused_index.set(Some(next));
            }
        } else if key == "Enter" {
            if let Some(url) = focused_url.get_untracked() {
                navigate_keydown(&url, NavigateOptions::default());
                set_search("".to_string());
                set_active(false);
                if let Some(input) = text_input.get() {
                    let _ = input.blur();
                }
            } else if let Some(first) = item_search().first() {
                navigate_keydown(&first.url, NavigateOptions::default());
                set_search("".to_string());
                set_active(false);
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
                    class="input w-full pl-10 pr-10"
                    type="text"
                    prop:value=search
                    aria-label="Search items"
                    aria-busy=move || loading().to_string()
                    aria-controls="search-results"
                    aria-expanded=move || active().to_string()
                    role="combobox"
                    aria-autocomplete="list"
                />
                <div class="absolute left-3 top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]">
                    <Icon icon=i::AiSearchOutlined />
                </div>
                <div class="absolute right-3 top-1/2 -translate-y-1/2">
                    <Show when=loading fallback=|| ()>
                        <Loading />
                    </Show>
                </div>
            </div>

            // Search Results
            <div
                id="search-results"
                role="listbox"
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

                            // Clone URL for different closures to satisfy borrow checker
                            let url_for_class = url.clone();
                            let url_for_click = url.clone();

                            view! {
                                <div
                                    class=move || {
                                        let hl = match focused_url.get() {
                                            Some(f) if f == url_for_class => " bg-[color:var(--color-background-elevated)]",
                                            _ => "",
                                        };
                                        format!("p-2 hover:bg-[color:var(--color-background-elevated)] cursor-pointer flex items-center gap-2{}", hl)
                                    }
                                    on:click=move |_| {
                                        navigate(&url_for_click, NavigateOptions::default());
                                        set_search("".to_string());
                                        set_active(false);
                                        if let Some(input) = text_input.get() {
                                            let _ = input.blur();
                                        }
                                    }
                                >
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
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {
                                                let category = result.category.clone();
                                                let result_type = result.result_type.clone();
                                                move || {
                                                    if let Some(cat) = &category {
                                                        if !cat.is_empty() {
                                                            format!("{} - {}", result_type, cat)
                                                        } else {
                                                            result_type.clone()
                                                        }
                                                    } else {
                                                        result_type.clone()
                                                    }
                                                }
                                            }
                                        </span>
                                    </div>
                                </div>
                            }
                        }
                        viewport_height=528.0
                        row_height=60.0
                        overscan=10
                        header_height=0.0
                        variable_height=false
                        scroll_to_index=Signal::derive(move || focused_index.get())

                    />
                </div>
            </div>
        </div>
    }
    .into_any()
}
