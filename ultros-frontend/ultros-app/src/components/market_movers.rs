//! Home-page Market Movers list with Rising / Falling / High Volume tabs.
//!
//! Each row shows: icon, item name, current price, %change pill, and an
//! inline 24h sparkline. Matches the section labeled "MARKET MOVRS" in
//! the dashboard mockup.
//!
//! Backed by `/api/v1/movers/{world}?direction=...&limit=...`. The server
//! returns 10 items with sparklines embedded — one round trip per tab.

use leptos::prelude::*;
use ultros_api_types::{icon_size::IconSize, sparklines::MoverItem};

use crate::{
    api::get_movers,
    components::{gil::Gil, item_icon::ItemIcon, sparkline::Sparkline},
    global_state::xiv_data::tracked_data,
    i18n::*,
};

/// Which mover bucket is selected. Frontend-only state; the strings are
/// the same direction values the API accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoverTab {
    Rising,
    Falling,
    Volume,
}

impl MoverTab {
    fn direction(self) -> &'static str {
        match self {
            MoverTab::Rising => "rising",
            MoverTab::Falling => "falling",
            MoverTab::Volume => "volume",
        }
    }
}

#[component]
fn MoverRow(item: MoverItem, world_name: String, index: usize) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = item.item_id;
    let name = tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| t_string!(i18n, unknown_item).to_string());

    let pct = item.pct_change_24h;
    let pct_class = if pct > 0.0 {
        "text-emerald-300"
    } else if pct < 0.0 {
        "text-red-300"
    } else {
        "text-[color:var(--color-text-muted)]"
    };
    let pct_text = if pct.abs() < 0.05 {
        "—".to_string()
    } else if pct >= 0.0 {
        format!("+{pct:.1}%")
    } else {
        format!("{pct:.1}%")
    };

    let row_bg = if index.is_multiple_of(2) {
        "bg-transparent"
    } else {
        "bg-[color:color-mix(in_srgb,var(--brand-ring)_4%,transparent)]"
    };

    view! {
        <a
            href=format!("/item/{}/{}", world_name, item_id)
            class=format!("group grid grid-cols-[auto_1fr_auto_auto_auto] items-center gap-3 px-3 py-2 rounded-lg {row_bg} hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] transition-colors")
        >
            <ItemIcon item_id icon_size=IconSize::Small />
            <div class="min-w-0">
                <div class="text-sm font-medium text-[color:var(--color-text)] truncate">
                    {name}
                </div>
                <div class="text-xs text-[color:var(--color-text-muted)] font-mono">
                    <Gil amount=item.price_now as i32 />
                </div>
            </div>
            <span class=format!("text-xs font-mono font-semibold {pct_class}")>
                {pct_text}
            </span>
            <Sparkline points=item.sparkline pct_change=pct />
            // Volume hidden on narrow rows — keeps the layout tight.
            <span class="hidden sm:inline text-xs font-mono text-[color:var(--color-text-muted)] min-w-[3rem] text-right">
                {format_volume(item.volume_24h)}
            </span>
        </a>
    }
}

/// Format unit volume for the right-aligned cell: 13520 -> "13.5K".
fn format_volume(v: u32) -> String {
    if v >= 1_000_000 {
        format!("{:.1}M", v as f64 / 1_000_000.0)
    } else if v >= 1_000 {
        format!("{:.1}K", v as f64 / 1_000.0)
    } else {
        v.to_string()
    }
}

#[component]
pub fn MarketMovers(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let (tab, set_tab) = signal(MoverTab::Rising);

    // Re-fetch on world or tab change. LocalResource = client-only so we
    // sidestep SSR hydration mismatches (same approach as MarketPulse).
    let movers = LocalResource::new(move || {
        let w = world.get();
        let dir = tab.get().direction();
        async move {
            let w = w?;
            get_movers(&w, dir, 10).await.ok()
        }
    });

    let tab_btn = move |this: MoverTab, label: AnyView| {
        let active = move || tab.get() == this;
        let active_class = "bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--color-text)] border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))]";
        let inactive_class = "bg-transparent text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] border-transparent";
        view! {
            <button
                type="button"
                class=move || format!(
                    "px-3 py-1.5 rounded-full text-xs font-semibold border transition-colors {}",
                    if active() { active_class } else { inactive_class }
                )
                on:click=move |_| set_tab.set(this)
            >
                {label}
            </button>
        }
    };

    view! {
        <section class="panel rounded-2xl p-4 sm:p-5 border border-[color:var(--color-outline)]">
            <header class="flex items-center justify-between gap-3 mb-3">
                <div>
                    <h2 class="text-sm uppercase tracking-wider text-[color:var(--color-text-muted)] flex items-center gap-2">
                        {t!(i18n, market_movers_title)}
                    </h2>
                    <p class="text-xs text-[color:var(--color-text-muted)] mt-0.5">
                        {t!(i18n, market_movers_subtitle)}
                    </p>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                    {tab_btn(MoverTab::Rising, t!(i18n, market_movers_rising).into_any())}
                    {tab_btn(MoverTab::Falling, t!(i18n, market_movers_falling).into_any())}
                    {tab_btn(MoverTab::Volume, t!(i18n, market_movers_volume).into_any())}
                </div>
            </header>

            <Suspense fallback=move || view! {
                <div class="space-y-1">
                    {(0..5).map(|_| view! {
                        <div class="h-12 rounded-lg bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    let w = world.get();
                    movers.get().map(|maybe| {
                        let world_name = w.unwrap_or_default();
                        // LocalResource here resolves to `Option<MoversResponse>`
                        // (Some on success, None on missing world / fetch error).
                        match maybe {
                            Some(resp) if !resp.items.is_empty() => view! {
                                <div class="flex flex-col gap-0.5">
                                    {resp.items
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, it)| view! {
                                            <MoverRow item=it world_name=world_name.clone() index=i />
                                        })
                                        .collect_view()}
                                </div>
                            }.into_any(),
                            _ => view! {
                                <div class="text-center text-sm text-[color:var(--color-text-muted)] py-8">
                                    {t!(i18n, market_movers_no_data)}
                                </div>
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </section>
    }
}
