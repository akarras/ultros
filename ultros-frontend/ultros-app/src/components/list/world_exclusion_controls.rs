use crate::components::icon::Icon;
use crate::global_state::LocalWorldData;
use crate::i18n::{t, use_i18n};
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashSet;
use ultros_api_types::world_helper::AnySelector;

#[component]
pub fn WorldExclusionControls(
    world_exclusions: RwSignal<HashSet<i32>>,
    available_worlds: Signal<Vec<i32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let world_data = use_context::<LocalWorldData>()
        .expect("LocalWorldData should be available")
        .0
        .expect("LocalWorldData should be loaded");

    let world_name = StoredValue::new(move |id: i32| {
        world_data
            .lookup_selector(AnySelector::World(id))
            .and_then(|r| r.as_world().map(|w| w.name.clone()))
            .unwrap_or_else(|| id.to_string())
    });

    let (show_selector, set_show_selector) = signal(false);

    view! {
        <div class="flex flex-col gap-2">
            <div class="flex flex-wrap items-center gap-2">
                <span class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                    {t!(i18n, list_view_excluded_worlds_label)}
                </span>
                <div class="flex flex-wrap gap-1">
                    {move || {
                        let exclusions = world_exclusions.get();
                        if exclusions.is_empty() {
                            Either::Left(view! {
                                <span class="text-xs text-[color:var(--color-text-muted)] italic">
                                    {t!(i18n, list_view_no_exclusions)}
                                </span>
                            })
                        } else {
                            Either::Right(exclusions.into_iter().map(move |id| {
                                let name = world_name.with_value(|f| f(id));
                                view! {
                                    <span class="inline-flex items-center gap-1 rounded-full bg-[color:var(--color-background-elevated)] border border-[color:var(--color-outline)] px-2 py-0.5 text-xs">
                                        <span>{name}</span>
                                        <button
                                            class="hover:text-red-400 transition-colors"
                                            on:click=move |_| {
                                                world_exclusions.update(|set| { set.remove(&id); });
                                            }
                                        >
                                            <Icon icon=i::BiXRegular />
                                        </button>
                                    </span>
                                }
                            }).collect_view())
                        }
                    }}
                </div>
                <button
                    class="btn-ghost p-1 text-[color:var(--brand-fg)]"
                    on:click=move |_| set_show_selector(!show_selector())
                >
                    <Icon icon=i::BiPlusRegular />
                </button>
                {move || if !world_exclusions.with(|s| s.is_empty()) {
                    view! {
                        <button
                            class="text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] underline"
                            on:click=move |_| { world_exclusions.update(|set| set.clear()); }
                        >
                            {t!(i18n, list_view_clear_exclusions)}
                        </button>
                    }.into_any()
                } else {
                    "".into_any()
                }}
            </div>

            <Show when=show_selector>
                <div class="flex flex-wrap gap-1 p-2 border border-[color:var(--color-outline)] rounded-lg bg-[color:var(--color-background)]/50">
                    {move || {
                        let current_exclusions = world_exclusions.get();
                        let mut worlds = available_worlds.get();
                        worlds.sort_by_key(|id| world_name.with_value(|f| f(*id)));
                        worlds.into_iter()
                            .filter(|id| !current_exclusions.contains(id))
                            .map(move |id| {
                                let name = world_name.with_value(|f| f(id));
                                view! {
                                    <button
                                        class="px-2 py-1 text-xs rounded hover:bg-[color:var(--brand-bg)]/20 border border-transparent hover:border-[color:var(--brand-ring)]/40 transition-colors"
                                        on:click=move |_| {
                                            world_exclusions.update(|set| { set.insert(id); });
                                        }
                                    >
                                        {name}
                                    </button>
                                }
                            }).collect_view()
                    }}
                </div>
            </Show>
        </div>
    }
}
