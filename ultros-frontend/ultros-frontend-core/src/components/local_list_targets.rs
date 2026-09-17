//! The "On this device" section of the add-to-list modals (`lists-sync` lab).
//!
//! Renders nothing unless [`use_local_lists`] resolves a bridge, so the modals
//! that embed it look exactly as before for everyone outside the lab. Inside
//! it, the section lists the device lists with an add button each, and ends
//! with a name field that creates a new device list and adds to it in one go
//! — the modal is the one place a player decides they need a list they
//! don't have yet.
//!
//! `build_items` is called at click time, so the modal's own controls (HQ,
//! quantity, per-ingredient counts) are read exactly when the add happens,
//! the same way the account-list buttons read them.
use std::collections::HashSet;

use leptos::either::EitherOf3;
use leptos::prelude::*;
use leptos::task::spawn_local;
use ultros_api_types::list::ListItem;
use ultros_ui::components::loading::Loading;

use crate::global_state::local_lists::{LocalListSummary, LocalLists, use_local_lists};
use crate::global_state::toasts::use_toast;
use crate::i18n::{t, t_string, use_i18n};

#[component]
pub fn LocalListTargets(
    /// The rows to add, built when the player clicks. Empty means nothing
    /// to add: existing lists ignore the click, a new list is still created.
    #[prop(into)]
    build_items: Callback<(), Vec<ListItem>>,
    /// Runs after a successful add (or create-and-add). The bulk modals use
    /// it to close themselves, matching their account-list behaviour.
    #[prop(optional, into)]
    on_added: Option<Callback<()>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let bridge = use_local_lists();
    let toasts = use_toast();

    let lists = RwSignal::new(None::<Result<Vec<LocalListSummary>, String>>);
    // Which lists this modal has already added to, so a row reads "added"
    // just like the account rows do, including a list created just now.
    let saved = RwSignal::new(HashSet::<String>::new());
    let running = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let name = RwSignal::new(String::new());
    let creating = RwSignal::new(false);

    Effect::new(move |_| {
        let Some(bridge) = bridge.get() else {
            return;
        };
        lists.set(None);
        spawn_local(async move {
            let result = (bridge.list)().await;
            let _ = lists.try_set(Some(result));
        });
    });

    let report = move |result: Result<(), String>, success: String| match result {
        Ok(()) => {
            if let Some(toasts) = toasts {
                toasts.success(success);
            }
            if let Some(on_added) = on_added {
                on_added.run(());
            }
        }
        Err(e) => {
            error.set(Some(e.clone()));
            if let Some(toasts) = toasts {
                toasts.error(format!("{} {e}", t_string!(i18n, add_to_list_error_toast)));
            }
        }
    };

    let add_to = move |bridge: LocalLists, id: String| {
        let items = build_items.run(());
        if items.is_empty() || running.get_untracked().is_some() {
            return;
        }
        error.set(None);
        running.set(Some(id.clone()));
        spawn_local(async move {
            let result = (bridge.add_items)(id.clone(), items).await;
            if result.is_ok() {
                saved.update(|s| {
                    s.insert(id);
                });
            }
            let _ = running.try_set(None);
            report(
                result,
                t_string!(i18n, add_to_list_success_toast).to_string(),
            );
        });
    };

    let create = move || {
        let Some(bridge) = bridge.get_untracked() else {
            return;
        };
        let list_name = name.get_untracked().trim().to_string();
        if list_name.is_empty() || creating.get_untracked() {
            return;
        }
        let items = build_items.run(());
        error.set(None);
        creating.set(true);
        spawn_local(async move {
            let result = async {
                let summary = (bridge.create)(list_name).await?;
                if !items.is_empty() {
                    (bridge.add_items)(summary.id.clone(), items).await?;
                }
                Ok(summary)
            }
            .await;
            let _ = creating.try_set(false);
            let result = result.map(|summary| {
                name.set(String::new());
                saved.update(|s| {
                    s.insert(summary.id.clone());
                });
                lists.update(|lists| {
                    if let Some(Ok(lists)) = lists {
                        lists.push(summary);
                    }
                });
            });
            report(
                result,
                t_string!(i18n, local_lists_created_toast).to_string(),
            );
        });
    };

    view! {
        <Show when=move || bridge.get().is_some()>
            <div class="space-y-1 pt-2" data-testid="local-list-targets">
                <div class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)] px-2">
                    {t!(i18n, local_lists_heading)}
                </div>
                {move || match lists.get() {
                    None => EitherOf3::A(view! { <Loading /> }),
                    Some(Err(e)) => EitherOf3::B(view! {
                        <div class="text-red-400 text-sm px-2">{e}</div>
                    }),
                    Some(Ok(lists)) => EitherOf3::C(view! {
                        <For
                            each=move || lists.clone()
                            key=|list| list.id.clone()
                            children=move |list| {
                                let id = StoredValue::new(list.id.clone());
                                let href = list.href();
                                let is_saved = move || saved.with(|s| id.with_value(|id| s.contains(id)));
                                let is_running = move || running.with(|r| id.with_value(|id| r.as_deref() == Some(id.as_str())));
                                let list_name = list.name.clone();
                                view! {
                                    <div class="flex items-center justify-between card p-2">
                                        <div class="flex items-center gap-2 min-w-0 flex-1 mr-2">
                                            <span class="font-semibold truncate">{list.name}</span>
                                            <span class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium border border-[color:var(--color-outline)] text-gray-300 shrink-0">
                                                {t!(i18n, local_lists_device_badge)}
                                            </span>
                                        </div>
                                        <Show when=is_saved>
                                            <a class="text-xs underline mr-2 shrink-0" href=href.clone()>
                                                {t!(i18n, online_open)}
                                            </a>
                                        </Show>
                                        <button
                                            type="button"
                                            class="btn-primary shrink-0"
                                            aria-label=format!("{} {list_name}", t_string!(i18n, add_to_list_aria_label_list))
                                            prop:disabled=move || running.get().is_some() || creating.get()
                                            on:click=move |_| {
                                                if let Some(bridge) = bridge.get_untracked() {
                                                    add_to(bridge, id.get_value());
                                                }
                                            }
                                        >
                                            {move || if is_saved() {
                                                EitherOf3::A(view! { <span>{t!(i18n, add_to_list_added_success)}</span> })
                                            } else if is_running() {
                                                EitherOf3::B(view! { <span>{t!(i18n, add_to_list_adding)}</span> })
                                            } else {
                                                EitherOf3::C(view! { <span>{t!(i18n, add_to_list_add)}</span> })
                                            }}
                                        </button>
                                    </div>
                                }
                            }
                        />
                    }),
                }}
                <form
                    class="flex items-center gap-2 card p-2"
                    on:submit=move |ev| {
                        ev.prevent_default();
                        create();
                    }
                >
                    <input
                        type="text"
                        class="input flex-1 min-w-0"
                        data-testid="local-list-new-name"
                        maxlength="100"
                        aria-label=move || t_string!(i18n, local_lists_new_name_label).to_string()
                        placeholder=move || t_string!(i18n, guest_workspace_placeholder).to_string()
                        prop:value=move || name.get()
                        prop:disabled=move || creating.get()
                        on:input=move |ev| name.set(event_target_value(&ev))
                    />
                    <button
                        type="submit"
                        class="btn-primary shrink-0"
                        data-testid="local-list-create-add"
                        prop:disabled=move || creating.get() || running.get().is_some() || name.get().trim().is_empty()
                    >
                        {move || if creating.get() {
                            t_string!(i18n, local_lists_creating).to_string()
                        } else {
                            t_string!(i18n, local_lists_create_and_add).to_string()
                        }}
                    </button>
                </form>
                <Show when=move || error.get().is_some()>
                    <div class="text-xs text-red-400 px-2">{move || error.get().unwrap_or_default()}</div>
                </Show>
            </div>
        </Show>
    }
}
