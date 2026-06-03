//! Home-page Market Movers list with Rising / Falling / Gil / Units tabs.
//!
//! Each row shows: icon, item name, current price, 24h % change, an inline
//! 24h sparkline, and a stacked volume cell — gil traded (price × quantity,
//! the "market value" metric) over the raw unit count. A column-header
//! legend labels every value and a footer sums the gil the listed movers
//! traded, so the numbers explain themselves.
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

/// Which mover bucket is selected. Frontend-only state; the strings map to
/// the `direction` values the API accepts. Rising/Falling rank by 24h price
/// change; Gil/Volume rank by gil traded / units traded respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoverTab {
    Rising,
    Falling,
    Gil,
    Volume,
}

impl MoverTab {
    fn direction(self) -> &'static str {
        match self {
            MoverTab::Rising => "rising",
            MoverTab::Falling => "falling",
            MoverTab::Gil => "gil",
            MoverTab::Volume => "volume",
        }
    }

    /// On the price-sorted tabs the sparkline color tracks price direction;
    /// on the volume-sorted tabs the chart is incidental, so it goes neutral.
    fn is_price_mode(self) -> bool {
        matches!(self, MoverTab::Rising | MoverTab::Falling)
    }
}

/// Shared grid template so the header legend and every data row line up
/// (only the name column flexes — every other track is a fixed width, which
/// keeps the separate header/row grids column-aligned). On narrow screens the
/// sparkline column drops out, leaving icon · name/price · Δ · volume.
const ROW_GRID: &str = "grid grid-cols-[1.5rem_minmax(0,1fr)_3.75rem_5.5rem] sm:grid-cols-[1.5rem_minmax(0,1fr)_3.75rem_5rem_5.5rem] items-center gap-3";

/// Tiny inline gil-coin glyph for compact gil figures. (`<Gil>`'s own icon is
/// an interactive party button — far too heavy for a dense stat cell, and we
/// don't want 10 of them firing confetti.)
fn gil_glyph() -> impl IntoView {
    view! {
        <img
            src="/static/images/gil.webp"
            alt=""
            aria-hidden="true"
            class="inline-block w-3 h-3 mr-0.5 align-[-2px]"
        />
    }
}

/// Format unit volume for the unit line: 13520 -> "13.5K".
fn format_volume(v: u32) -> String {
    if v >= 1_000_000 {
        format!("{:.1}M", v as f64 / 1_000_000.0)
    } else if v >= 1_000 {
        format!("{:.1}K", v as f64 / 1_000.0)
    } else {
        v.to_string()
    }
}

/// Format gil volume compactly: 4_230_000_000 -> "4.2B". Gil traded reaches
/// the billions on busy items, so this carries an extra magnitude over
/// [`format_volume`].
fn format_gil_compact(v: u64) -> String {
    let f = v as f64;
    if f >= 1_000_000_000.0 {
        format!("{:.1}B", f / 1_000_000_000.0)
    } else if f >= 1_000_000.0 {
        format!("{:.1}M", f / 1_000_000.0)
    } else if f >= 1_000.0 {
        format!("{:.1}K", f / 1_000.0)
    } else {
        v.to_string()
    }
}

#[component]
fn MoverRow(item: MoverItem, world_name: String, tab: MoverTab) -> impl IntoView {
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
    // Emphasize the %change on the price-sorted tabs — that's what the list
    // is ranked by there.
    let pct_weight = if tab.is_price_mode() {
        " font-semibold"
    } else {
        ""
    };
    let pct_cell_class =
        format!("text-xs font-mono text-right whitespace-nowrap {pct_class}{pct_weight}");

    // Sparkline tracks price direction only on the price-sorted tabs.
    let spark_pct = if tab.is_price_mode() { pct } else { 0.0 };

    // The volume cell always shows both metrics stacked (gil over units), so
    // you never have to guess whether a number is gil or items. Whichever the
    // list is sorted by gets the brighter weight; on the price tabs gil — the
    // value metric — leads.
    let units_active = tab == MoverTab::Volume;
    let gil_line_class = if units_active {
        "text-xs font-mono text-[color:var(--color-text-muted)] whitespace-nowrap"
    } else {
        "text-xs font-mono font-semibold text-[color:var(--color-text)] whitespace-nowrap"
    };
    let units_line_class = if units_active {
        "text-[11px] font-mono font-semibold text-[color:var(--color-text)] whitespace-nowrap"
    } else {
        "text-[11px] font-mono text-[color:var(--color-text-muted)] whitespace-nowrap"
    };

    view! {
        <a
            href=format!("/item/{}/{}", world_name, item_id)
            class=format!("{ROW_GRID} group px-1 py-2 border-b border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] transition-colors")
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
            <span class=pct_cell_class>{pct_text}</span>
            // Sparkline is the most space-hungry, least dense cell — it drops
            // out first on narrow rows so the gil/units figures keep room.
            <span class="hidden sm:block">
                <Sparkline points=item.sparkline pct_change=spark_pct />
            </span>
            <div class="text-right leading-tight">
                <div class=gil_line_class>
                    {gil_glyph()}{format_gil_compact(item.gil_volume_24h)}
                </div>
                <div class=units_line_class>
                    {format_volume(item.volume_24h)}" "{t!(i18n, market_movers_units_suffix)}
                </div>
            </div>
        </a>
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

    let tab_btn = move |this: MoverTab, label: AnyView, tooltip: String| {
        let active = move || tab.get() == this;
        let active_class = "bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--color-text)] border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))]";
        let inactive_class = "bg-transparent text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] border-transparent";
        view! {
            <button
                type="button"
                title=tooltip
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
        <section class="dashboard-section">
            <header class="flex items-baseline justify-between gap-3 mb-3 flex-wrap">
                <div class="flex items-baseline gap-3 flex-wrap">
                    <h2 class="dashboard-section-title">
                        {t!(i18n, market_movers_title)}
                    </h2>
                    <p class="text-xs text-[color:var(--color-text-muted)]">
                        {t!(i18n, market_movers_subtitle)}
                    </p>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                    {tab_btn(
                        MoverTab::Rising,
                        t!(i18n, market_movers_rising).into_any(),
                        t_string!(i18n, market_movers_tab_rising_help).to_string(),
                    )}
                    {tab_btn(
                        MoverTab::Falling,
                        t!(i18n, market_movers_falling).into_any(),
                        t_string!(i18n, market_movers_tab_falling_help).to_string(),
                    )}
                    {tab_btn(
                        MoverTab::Gil,
                        t!(i18n, market_movers_gil).into_any(),
                        t_string!(i18n, market_movers_tab_gil_help).to_string(),
                    )}
                    {tab_btn(
                        MoverTab::Volume,
                        t!(i18n, market_movers_units).into_any(),
                        t_string!(i18n, market_movers_tab_units_help).to_string(),
                    )}
                </div>
            </header>

            // Column legend — labels every value so the figures are
            // self-describing. Sparkline header is blank (a trend line needs
            // no caption) and hidden alongside its column on narrow screens.
            <div class=format!("{ROW_GRID} px-1 pb-1.5 mb-0.5 text-[10px] font-semibold uppercase tracking-wider text-[color:var(--color-text-muted)] border-b border-[color:var(--line)]")>
                <span></span>
                <span>{t!(i18n, market_movers_col_item)}</span>
                <span class="text-right">{t!(i18n, market_movers_col_change)}</span>
                <span class="hidden sm:block"></span>
                <span class="text-right">{t!(i18n, market_movers_col_volume)}</span>
            </div>

            <Suspense fallback=move || view! {
                <div class="space-y-1">
                    {(0..5).map(|_| view! {
                        <div class="h-12 rounded-lg bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    let w = world.get();
                    let current_tab = tab.get();
                    movers.get().map(|maybe| {
                        let world_name = w.unwrap_or_default();
                        // LocalResource here resolves to `Option<MoversResponse>`
                        // (Some on success, None on missing world / fetch error).
                        match maybe {
                            Some(resp) if !resp.items.is_empty() => {
                                // Footer total: how much gil the listed movers
                                // traded in the window — a literal "total
                                // market value" readout for the section.
                                let total_gil: u64 =
                                    resp.items.iter().map(|i| i.gil_volume_24h).sum();
                                let shown = resp.items.len();
                                view! {
                                    <div>
                                        <div class="flex flex-col">
                                            {resp.items
                                                .into_iter()
                                                .map(|it| view! {
                                                    <MoverRow
                                                        item=it
                                                        world_name=world_name.clone()
                                                        tab=current_tab
                                                    />
                                                })
                                                .collect_view()}
                                        </div>
                                        <div class="flex items-center justify-end gap-1.5 px-1 pt-2 text-xs text-[color:var(--color-text-muted)]">
                                            <span>{t!(i18n, market_movers_total_gil, count = shown)}</span>
                                            <span class="font-mono font-semibold text-[color:var(--color-text)]">
                                                {gil_glyph()}{format_gil_compact(total_gil)}
                                            </span>
                                        </div>
                                    </div>
                                }.into_any()
                            }
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

#[cfg(test)]
mod test_formatters {
    use super::*;

    #[test]
    fn test_format_volume() {
        assert_eq!(format_volume(0), "0");
        assert_eq!(format_volume(999), "999");
        assert_eq!(format_volume(1000), "1.0K");
        assert_eq!(format_volume(1500), "1.5K");
        assert_eq!(format_volume(999_999), "1000.0K");
        assert_eq!(format_volume(1_000_000), "1.0M");
        assert_eq!(format_volume(1_500_000), "1.5M");
    }

    #[test]
    fn test_format_gil_compact() {
        assert_eq!(format_gil_compact(0), "0");
        assert_eq!(format_gil_compact(999), "999");
        assert_eq!(format_gil_compact(1_000), "1.0K");
        assert_eq!(format_gil_compact(12_400), "12.4K");
        assert_eq!(format_gil_compact(1_000_000), "1.0M");
        assert_eq!(format_gil_compact(12_400_000), "12.4M");
        assert_eq!(format_gil_compact(1_000_000_000), "1.0B");
        assert_eq!(format_gil_compact(4_230_000_000), "4.2B");
    }
}
