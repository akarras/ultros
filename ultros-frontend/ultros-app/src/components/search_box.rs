use crate::components::icon::Icon;
use crate::components::loading::Loading;
use crate::components::virtual_scroller::*;
use gloo_timers::future::TimeoutFuture;
use icondata as i;
use leptos::{html::Input, prelude::*, task::spawn_local};
use leptos_hotkeys::use_hotkeys;
use leptos_router::{NavigateOptions, hooks::use_navigate};
use std::sync::Arc;
use std::sync::LazyLock;
use ultros_api_types::search::SearchResult;
use web_sys::KeyboardEvent;

static STATIC_PAGES: LazyLock<Vec<SearchResult>> = LazyLock::new(|| {
    vec![
        SearchResult {
            score: 100.0,
            title: "Flip Finder".to_string(),
            result_type: "Tool".to_string(),
            url: "/analyzer".to_string(),
            icon_id: None,
            category: Some("Market Analysis".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "Recipe Analyzer".to_string(),
            result_type: "Tool".to_string(),
            url: "/recipe-analyzer".to_string(),
            icon_id: None,
            category: Some("Crafting".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "Leve Analyzer".to_string(),
            result_type: "Tool".to_string(),
            url: "/leve-analyzer".to_string(),
            icon_id: None,
            category: Some("Leveling".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "Currency Exchange".to_string(),
            result_type: "Tool".to_string(),
            url: "/currency-exchange".to_string(),
            icon_id: None,
            category: Some("Currencies".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "My Lists".to_string(),
            result_type: "Page".to_string(),
            url: "/lists".to_string(),
            icon_id: None,
            category: Some("Personal".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "Retainers".to_string(),
            result_type: "Page".to_string(),
            url: "/retainers".to_string(),
            icon_id: None,
            category: Some("Personal".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "Settings".to_string(),
            result_type: "Page".to_string(),
            url: "/settings".to_string(),
            icon_id: None,
            category: Some("System".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "History".to_string(),
            result_type: "Page".to_string(),
            url: "/history".to_string(),
            icon_id: None,
            category: Some("Personal".to_string()),
        },
        SearchResult {
            score: 100.0,
            title: "Alerts".to_string(),
            result_type: "Page".to_string(),
            url: "/alerts".to_string(),
            icon_id: None,
            category: Some("Personal".to_string()),
        },
    ]
});

fn get_static_pages() -> &'static [SearchResult] {
    &STATIC_PAGES
}

#[component]
pub fn SearchBox() -> impl IntoView {
    let text_input = NodeRef::<Input>::new();
    let (search, set_search) = signal(String::new());
    let navigate = use_navigate();
    let (active, set_active) = signal(false);
    let (loading, set_loading) = signal(false);

    use crate::api::search as api_search;

    // Search results and request tracking
    let (search_results, set_search_results) = signal::<Vec<Arc<SearchResult>>>(Vec::new());
    let (search_id, set_search_id) = signal(0usize);

    // Keyboard navigation focus handling
    let (focused_index, set_focused_index) = signal::<Option<usize>>(None);

    // Currently-focused result's URL for highlight/selection
    let focused_url: Signal<Option<String>> = Signal::derive(move || {
        focused_index.get().and_then(|idx| {
            search_results.with(|v: &Vec<Arc<SearchResult>>| {
                v.get(idx).map(|r: &Arc<SearchResult>| r.url.clone())
            })
        })
    });

    // Helper to generate a safe DOM ID from a URL
    let get_id_from_url =
        |url: &str| format!("search-result-{}", url.replace(['/', ':', '.'], "-"));

    // When results change, reset the focused index to the first item (if any)
    Effect::new(move |_| {
        let len = search_results.with(|v: &Vec<Arc<SearchResult>>| v.len());
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

            let s_lower = s.to_lowercase();
            let mut matched_pages: Vec<SearchResult> = get_static_pages()
                .iter()
                .filter(|p| p.title.to_lowercase().contains(&s_lower))
                .cloned()
                .collect();

            // Sort matched pages so exact matches or starts_with come first
            matched_pages.sort_by(|a, b| {
                let a_starts = a.title.to_lowercase().starts_with(&s_lower);
                let b_starts = b.title.to_lowercase().starts_with(&s_lower);
                b_starts.cmp(&a_starts) // true (starts with) comes first
            });

            set_loading.set(true);
            match api_search(&s).await {
                Ok(mut results) => {
                    if search_id.get_untracked() == current_id {
                        // Prepend static pages to the backend results
                        let mut final_results = matched_pages;
                        final_results.append(&mut results);

                        let results = final_results.into_iter().map(Arc::new).collect();
                        set_search_results.set(results);
                        set_loading.set(false);
                    }
                }
                Err(e) => {
                    if search_id.get_untracked() == current_id {
                        log::error!("Search failed: {}", e);
                        // Even if backend fails, show matched static pages
                        let results = matched_pages.into_iter().map(Arc::new).collect();
                        set_search_results.set(results);
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
            let len = search_results.with_untracked(|v: &Vec<Arc<SearchResult>>| v.len());
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
            let len = search_results.with_untracked(|v: &Vec<Arc<SearchResult>>| v.len());
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
            } else {
                let first_url = search_results.with_untracked(|r| r.first().map(|f| f.url.clone()));
                if let Some(url) = first_url {
                    navigate_keydown(&url, NavigateOptions::default());
                    set_search("".to_string());
                    set_active(false);
                    if let Some(input) = text_input.get() {
                        let _ = input.blur();
                    }
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
                    aria-activedescendant=move || {
                        focused_url
                            .get()
                            .map(|url| get_id_from_url(&url))
                            .unwrap_or_default()
                    }
                />
                <div class="absolute left-3 top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]">
                    <Show when=loading fallback=|| view! { <Icon icon=i::MdiJellyfish /> }>
                        <Loading />
                    </Show>
                </div>
                <div class="absolute right-3 top-1/2 -translate-y-1/2">
                    <Show when=move || !search.get().is_empty()>
                        <button
                            class="text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors"
                            on:click=move |_| {
                                set_search("".to_string());
                                if let Some(input) = text_input.get() {
                                    let _ = input.focus();
                                }
                                set_active(true);
                            }
                            aria-label="Clear search"
                        >
                            <Icon icon=i::BsX width="1.5em" height="1.5em" aria_hidden=true />
                        </button>
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
                        each=search_results.into()
                        key={move |result: &Arc<SearchResult>| result.url.clone()}
                        view={move |result: Arc<SearchResult>| {
                            let url = result.url.clone();
                            let navigate = navigate.clone();

                            // Clone URL for different closures to satisfy borrow checker
                            let url_for_class = url.clone();
                            let url_for_aria = url.clone();
                            let url_for_click = url.clone();
                            let url_for_id = url.clone();

                            view! {
                                <div
                                    id=get_id_from_url(&url_for_id)
                                    role="option"
                                    aria-selected=move || {
                                        match focused_url.get() {
                                            Some(f) if f == url_for_aria => "true",
                                            _ => "false",
                                        }
                                    }
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
                                                let (failed, set_failed) = signal(false);
                                                let result_title = result.title.clone();
                                                view! {
                                                    <div class="w-8 h-8 flex-shrink-0">
                                                        <img
                                                            src=move || {
                                                                if failed.get() {
                                                                    "/static/itemicon/fallback".to_string()
                                                                } else {
                                                                    format!("/static/itemicon/{}?size=Small", icon_id)
                                                                }
                                                            }
                                                            alt=move || format!("Icon for {}", result_title)
                                                            class="w-full h-full object-contain"
                                                            loading="lazy"
                                                            on:error=move |_| set_failed.set(true)
                                                        />
                                                    </div>
                                                }.into_any()
                                            } else {
                                                match result.result_type.as_str() {
                                                    "item" => view! { <Icon icon=i::FaBoxOpenSolid /> }.into_any(),
                                                    "currency" => view! { <Icon icon=i::FaCoinsSolid /> }.into_any(),
                                                    "category" => view! { <Icon icon=i::FaListSolid /> }.into_any(),
                                                    "job equipment" => view! { <Icon icon=i::FaUserSolid /> }.into_any(),
                                                    "Tool" => view! { <Icon icon=i::FaWrenchSolid /> }.into_any(),
                                                    "Page" => view! { <Icon icon=i::AiFileTextOutlined /> }.into_any(),
                                                    _ => view! { <Icon icon=i::MdiJellyfish /> }.into_any(),
                                                }
                                            }
                                        } else {
                                            match result.result_type.as_str() {
                                                "item" => view! { <Icon icon=i::FaBoxOpenSolid /> }.into_any(),
                                                "currency" => view! { <Icon icon=i::FaCoinsSolid /> }.into_any(),
                                                "category" => view! { <Icon icon=i::FaListSolid /> }.into_any(),
                                                "job equipment" => view! { <Icon icon=i::FaUserSolid /> }.into_any(),
                                                "Tool" => view! { <Icon icon=i::FaWrenchSolid /> }.into_any(),
                                                "Page" => view! { <Icon icon=i::AiFileTextOutlined /> }.into_any(),
                                                _ => view! { <Icon icon=i::MdiJellyfish /> }.into_any(),
                                            }
                                        }
                                    }
                                    <div class="flex flex-col">
                                        <span class="font-medium">{result.title.clone()}</span>
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
                        }}
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
