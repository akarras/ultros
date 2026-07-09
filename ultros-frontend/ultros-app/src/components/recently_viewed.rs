use std::collections::{HashMap, VecDeque};

use crate::api::post_sparklines;
use crate::global_state::home_world::use_home_world;
use crate::global_state::xiv_data::tracked_data;
use codee::string::JsonSerdeCodec;
use leptos::leptos_dom::helpers::set_timeout;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::sparklines::{SparklineSeries, SparklinesRequest};
use xiv_gen::ItemId;

use crate::components::{
    gil::Gil, item_icon::ItemIcon, skeleton::BoxSkeleton, sparkline::Sparkline,
};
use crate::i18n::*;

#[derive(Clone, Copy)]
pub struct RecentItems {
    read_signal: Signal<VecDeque<i32>>,
    write_signal: WriteSignal<VecDeque<i32>>,
}

impl RecentItems {
    pub fn new() -> Self {
        // `delay_during_hydration` is required: without it leptos-use reads
        // localStorage synchronously inside the component setup, which on the
        // client races the still-running hydration. The CSR signal then holds
        // a non-empty VecDeque while the SSR'd DOM was rendered with the
        // empty default, so tachys' walker hits a different element shape at
        // the `RecentlyViewed` slot and panics at `hydration.rs:163`
        // (`failed_to_cast_element`). Deferring the storage read to the next
        // animation frame lets hydration finish with matching shapes first,
        // then the rail repopulates reactively. Drove the homepage panics
        // (GlitchTip 3147 + 4327) and contributed to item-page cascades.
        let (read_signal, write_signal, _delete_fn) =
            use_local_storage_with_options::<VecDeque<i32>, JsonSerdeCodec>(
                "recently_viewed",
                UseStorageOptions::default().delay_during_hydration(true),
            );
        Self {
            read_signal,
            write_signal,
        }
    }

    pub fn reader(&self) -> Signal<VecDeque<i32>> {
        self.read_signal
    }

    pub fn add_item(&self, item_id: i32) {
        self.write_signal.update(|items| {
            // ⚡ Bolt Optimization:
            // Previously used `items.iter().copied().unique().collect()` which allocated a
            // new collection and performed expensive hashing for up to 1000 items on every view.
            // Now we do a fast O(N) linear scan and in-place shift, which is nearly instant
            // and avoids allocations. We also fast-path the common case of refreshing the same item.
            if let Some(pos) = items.iter().position(|&id| id == item_id) {
                if pos == 0 {
                    return; // Fast path: already at the front
                }
                items.remove(pos);
            }
            items.push_front(item_id);
            if items.len() > 1000 {
                items.pop_back();
            }
        });
    }

    pub fn clear_items(&self) {
        self.write_signal.update(|items| items.clear());
    }
}

fn format_pct_change(pct_change: Option<f32>) -> (&'static str, String) {
    let pct_class = match pct_change {
        Some(p) if p > 0.05 => "text-emerald-300",
        Some(p) if p < -0.05 => "text-red-300",
        Some(_) => "text-[color:var(--color-text-muted)]",
        None => "text-[color:var(--color-text-muted)]",
    };
    let pct_text = match pct_change {
        Some(p) if p.abs() < 0.05 => "—".to_string(),
        Some(p) if p >= 0.0 => format!("+{p:.1}%"),
        Some(p) => format!("{p:.1}%"),
        None => "—".to_string(),
    };
    (pct_class, pct_text)
}

/// One row in the Continue Tracking panel — item icon, name, current
/// price, %change pill, and inline 24h sparkline.
#[component]
fn TrackedRow(item_id: i32, world_name: String, series: Option<SparklineSeries>) -> impl IntoView {
    let item_data = tracked_data().items.get(&ItemId(item_id));
    let name = item_data
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_default();

    let (pct_change, last_price, points): (Option<f32>, Option<u32>, Vec<u32>) = match series {
        Some(s) => {
            // Compute pct change from first/last endpoints. Guard against
            // zero first_price — those rows show "—" instead of garbage.
            let pct = if s.first_price > 0 {
                Some(((s.last_price as f32 - s.first_price as f32) / s.first_price as f32) * 100.0)
            } else {
                None
            };
            (pct, Some(s.last_price), s.points)
        }
        None => (None, None, Vec::new()),
    };

    let (pct_class, pct_text) = format_pct_change(pct_change);

    // The /item/{world}/{id} route is the canonical product page when we
    // have a world. Without a world, fall back to /item/{id}.
    let href = if world_name.is_empty() {
        format!("/item/{item_id}")
    } else {
        format!("/item/{world_name}/{item_id}")
    };

    view! {
        <A href=href>
            <div class="grid grid-cols-[auto_1fr_auto_auto] items-center gap-2 px-1 py-2 border-b border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--accent)_6%,transparent)] transition-colors rounded">
                <ItemIcon item_id icon_size=IconSize::Small />
                <div class="min-w-0">
                    <div class="text-sm text-[color:var(--color-text)] truncate">{name}</div>
                    <div class="text-[10px] font-mono text-[color:var(--color-text-muted)] leading-tight">
                        {last_price.map(|p| view! { <Gil amount=p as i32 /> })}
                    </div>
                </div>
                <span class=format!("text-xs font-mono font-semibold tabular-nums {pct_class}")>
                    {pct_text}
                </span>
                {(!points.is_empty()).then(|| view! {
                    <Sparkline points pct_change=pct_change.unwrap_or(0.0) />
                })}
            </div>
        </A>
    }
}

#[component]
pub fn RecentlyViewed() -> impl IntoView {
    let i18n = use_i18n();
    let item_data = use_context::<RecentItems>().unwrap();
    let items = item_data.reader();
    let (homeworld, _) = use_home_world();
    // Limit to top 8 to keep the rail focused. The /history page is the
    // overflow surface for everything older.
    let recent_top: Signal<Vec<i32>> =
        Signal::derive(move || items.with(|q| q.iter().take(8).copied().collect()));
    let world_name: Signal<Option<String>> =
        Signal::derive(move || homeworld.with(|w| w.as_ref().map(|w| w.name.clone())));

    // Fetch sparklines for the visible top-N items. Re-runs when either
    // home world or the recently-viewed list changes.
    let sparklines = LocalResource::new(move || {
        let world = world_name.get();
        let ids = recent_top.get();
        async move {
            let world = world?;
            if ids.is_empty() {
                return None;
            }
            let req = SparklinesRequest {
                items: ids.into_iter().map(|id| (id, false)).collect(),
                hours: Some(24),
            };
            post_sparklines(&world, req).await.ok()
        }
    });

    let (confirm_clear, set_confirm_clear) = signal(false);

    view! {
        <div class="py-2">
            <Suspense fallback=move || {
                view! {
                    <div class="h-[280px] animate-pulse">
                        <BoxSkeleton />
                    </div>
                }
            }>
                <div
                    class=""
                    class:hidden=move || recent_top.with(|i| i.is_empty())
                >
                    <div class="flex items-baseline justify-between mb-2">
                        <h4 class="dashboard-section-title">{t!(i18n, recently_viewed_title)}</h4>
                        <button
                            class="text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors focus:outline-none focus:ring-2 focus:ring-[color:var(--accent)] rounded px-1"
                            on:click=move |_| {
                                if confirm_clear.get_untracked() {
                                    item_data.clear_items();
                                    set_confirm_clear(false);
                                } else {
                                    set_confirm_clear(true);
                                    set_timeout(
                                        move || set_confirm_clear(false),
                                        std::time::Duration::from_secs(3),
                                    );
                                }
                            }
                        >
                            {move || {
                                if confirm_clear.get() {
                                    t_string!(i18n, recently_viewed_confirm_clear).to_string()
                                } else {
                                    t_string!(i18n, recently_viewed_clear_all).to_string()
                                }
                            }}
                        </button>
                    </div>

                    <div class="max-h-[420px] overflow-y-auto overflow-x-hidden scrollbar-thin">
                        {move || {
                            let ids = recent_top.get();
                            if ids.is_empty() {
                                return None;
                            }
                            // Build series lookup so each row picks up its
                            // sparkline without a second list scan. When the
                            // request fails or world is unset, the map is
                            // empty and rows just render without sparklines.
                            // LocalResource resolves to Option<Option<SparklinesResponse>>.
                            // The outer Option means "has the resource resolved", the
                            // inner Option is the body itself (None on missing world or
                            // fetch error). Flatten to Option<SparklinesResponse>.
                            let series_map: HashMap<i32, SparklineSeries> = sparklines
                                .get()
                                .flatten()
                                .map(|resp| {
                                    resp.series
                                        .into_iter()
                                        .map(|s| (s.item_id, s))
                                        .collect()
                                })
                                .unwrap_or_default();
                            let world = world_name.get().unwrap_or_default();
                            Some(
                                ids.into_iter()
                                    .map(|item_id| {
                                        let series = series_map.get(&item_id).cloned();
                                        view! {
                                            <TrackedRow
                                                item_id
                                                world_name=world.clone()
                                                series
                                            />
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        }}
                    </div>

                    <div class="text-right pt-2">
                        <a
                            href="/history"
                            class="text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--accent)] transition-colors"
                        >
                            {t!(i18n, recently_viewed_view_all)}
                        </a>
                    </div>
                </div>
            </Suspense>
        </div>
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_pct_change() {
        // None case
        assert_eq!(
            format_pct_change(None),
            ("text-[color:var(--color-text-muted)]", "—".to_string())
        );

        // Near zero (within 0.05 bounds)
        assert_eq!(
            format_pct_change(Some(0.0)),
            ("text-[color:var(--color-text-muted)]", "—".to_string())
        );
        assert_eq!(
            format_pct_change(Some(0.04)),
            ("text-[color:var(--color-text-muted)]", "—".to_string())
        );
        assert_eq!(
            format_pct_change(Some(-0.04)),
            ("text-[color:var(--color-text-muted)]", "—".to_string())
        );

        // Positive delta (> 0.05)
        assert_eq!(
            format_pct_change(Some(0.06)),
            ("text-emerald-300", "+0.1%".to_string()) // 0.06 rounds to 0.1
        );
        assert_eq!(
            format_pct_change(Some(5.42)),
            ("text-emerald-300", "+5.4%".to_string())
        );

        // Negative delta (< -0.05)
        assert_eq!(
            format_pct_change(Some(-0.06)),
            ("text-red-300", "-0.1%".to_string())
        );
        assert_eq!(
            format_pct_change(Some(-12.8)),
            ("text-red-300", "-12.8%".to_string())
        );
    }
}
