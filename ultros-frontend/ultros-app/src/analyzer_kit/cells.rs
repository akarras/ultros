//! The kit's cell vocabulary: a small value enum rendered by one match,
//! so per-variant markup lives in exactly one place and every
//! resource-backed variant keeps one DOM shape across its states.

use leptos::prelude::*;
use leptos_i18n::I18nContext;
use thousands::Separable;
use ultros_api_types::trends::ConfidenceBand;

use crate::analysis::{DELTA_DEAD_BAND_PCT, roi_badge_class, signed_delta_class};
use crate::components::confidence_badge::ConfidenceBadge;
use crate::components::gil::{Gil, GilIcon, GilOrDash};
use crate::components::sparkline::Sparkline;
use crate::components::term_badge::TermRole;
use crate::i18n::*;

use super::columns::CellCtx;
use super::enrichment::SparkValue;
use super::hop::HopGain;

/// The three states a resource-backed cell can be in: the fetch has not
/// answered for this key yet, it answered with nothing, or it answered.
/// `Missing` and `Ready` are settled — only `Loading` shimmers, and it is
/// what the server and the first client paint always render (the stores are
/// empty on both sides), which is what keeps hydration honest.
#[derive(Clone, Debug, PartialEq)]
pub enum Enrich<V> {
    Loading,
    Missing,
    Ready(V),
}

impl<V> Enrich<V> {
    /// Map the payload, keeping the state. Turns the borrowed
    /// `Enrich<&V>` a store read yields into the owned value a cell holds.
    pub fn map<U>(self, f: impl FnOnce(V) -> U) -> Enrich<U> {
        match self {
            Enrich::Loading => Enrich::Loading,
            Enrich::Missing => Enrich::Missing,
            Enrich::Ready(v) => Enrich::Ready(f(v)),
        }
    }

    /// Whether the cell shows its skeleton — the one state difference the
    /// one-shape rule lets a cell branch a class on.
    pub fn is_loading(&self) -> bool {
        matches!(self, Enrich::Loading)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Gil(i32),
    RoiBadge(i32),
    Count(u64),
    Confidence(ConfidenceBand),
    LastSoldUnix(i64),
    /// A gil amount with a percent sub-line (VWAP and its % vs price).
    /// `amount <= 0` renders the dash; the sub-line is always present.
    GilWithPct {
        amount: i32,
        pct: Option<f32>,
    },
    /// An alternative-signal amount: muted, with an always-present 10px
    /// sub-line holding the delta against the same-side formula input.
    /// `capped` = the sub-craft cap left it unpriced (a different title).
    MutedGil {
        amount: Option<i32>,
        pct: Option<f32>,
        side: TermRole,
        capped: bool,
    },
    /// A gil amount with an always-present note sub-line (the Price slot's
    /// "listing" fallback tell).
    GilWithNote {
        amount: i32,
        note: CellNote,
    },
    /// A lazily fetched hourly price series, coloured by its own
    /// first-to-last percent.
    Sparkline(Enrich<SparkValue>),
    /// A lazily fetched signed percent (Drift). `Ready(None)` means the
    /// series had no first trade, so no percentage exists — it reads like
    /// `Missing`, with the same "not enough sales" tell.
    LazyPct(Enrich<Option<f32>>),
    /// A count from a body that lands after the table (Volume 30d).
    LateCount(Enrich<u64>),
    /// A gil amount and its percent against Price, from a body that lands
    /// after the table (VWAP 30d).
    LateGilWithPct(Enrich<(i32, Option<f32>)>),
    /// Hop gain / unit: signed gil, the word "needed", or the dash, in one
    /// shape; `daily_sales` feeds the gil/day title.
    Hop {
        gain: HopGain,
        daily_sales: f32,
    },
    /// A signed gil delta against a baseline, with an always-present
    /// percent sub-line: Scope vs home. **Not** `MutedGil` — that one
    /// filters `amount > 0`, and a negative delta is this column's normal
    /// state (a wider market can only undercut under the cheapest
    /// listing). `pct: None` renders the value uncoloured, which is what
    /// the one-sided listing case wants: the sign is the whole message and
    /// a permanent red stripe teaches readers to ignore the colour.
    /// `unavailable` titles the dash with the reason, the way
    /// [`CellValue::LazyPct`]'s empty state does.
    SignedGil {
        delta: Option<i32>,
        pct: Option<f32>,
        unavailable: bool,
    },
    /// The page renders this cell itself.
    Custom,
}

/// The sub-line under a [`CellValue::GilWithNote`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CellNote {
    None,
    /// The price fell back to a listing (the selected signal had no row on
    /// the sell world, or the sell world had no listing at all).
    ListingFallback,
    /// This price against the sell world's 7-day sale median, signed and
    /// coloured; `listing` keeps the fallback tell in front of it, so the
    /// line reads `listing · vs median +4%`.
    VsMedian {
        listing: bool,
        pct: f32,
    },
    /// The price clears [`crate::analysis::is_troll_listing`] against that
    /// same median: the rest of the analyzer calls this a troll listing and
    /// refuses to price against it, so the tell says so in the warning
    /// colour instead of painting a four-digit percentage emerald.
    Troll {
        listing: bool,
    },
}

/// "13.5k", "632", "1.5M": the gil/day figure in a hop title.
pub fn gil_per_day_label(gil: f32) -> String {
    let abs = gil.abs();
    if abs >= 1_000_000.0 {
        format!("{:.1}M", gil / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}k", gil / 1_000.0)
    } else {
        format!("{gil:.0}")
    }
}

fn signed_gil(g: i32) -> String {
    if g > 0 {
        format!("+{}", g.separate_with_commas())
    } else {
        g.separate_with_commas()
    }
}

const SUB_LINE: &str = "text-[10px] leading-3 text-[color:var(--color-text-muted)]";
/// The geometry half of [`SUB_LINE`], so a coloured sub-line can compose it
/// with `signed_delta_class`. Inside the dead band that composition is
/// `SUB_LINE` character for character, which is what keeps the Price note
/// identical for the states that predate the median tell.
const SUB_LINE_GEOM: &str = "text-[10px] leading-3";

/// The Price note's warning colour. The same class `signed_delta_class`
/// returns below its dead band — named here only because the troll tell has
/// no percentage to hand it — and pinned equal to it in
/// `the_price_note_adds_the_median_tell_without_moving_phase_d`.
const SUB_LINE_WARN: &str = "text-red-300";

/// The bar a lazy or late cell shows while its fetch is in flight. Inline
/// rather than `SingleLineSkeleton`: one shape needs the element present in
/// every state, and that component's `sr-only` "Loading…" would then be
/// announced on settled rows.
const SKELETON_BAR: &str = "skeleton-block skeleton-shimmer w-full h-3 rounded-md";

fn bar_class(loading: bool) -> &'static str {
    if loading { SKELETON_BAR } else { "hidden" }
}

/// Resting label for the last-sold cell — same day/hour/just-now buckets
/// and i18n keys as the flip finder's `COL_LAST_SOLD` cell. A zero or
/// future timestamp renders as "never" (no sale in the window / old
/// server).
pub fn last_sold_label(
    i18n: I18nContext<Locale, I18nKeys>,
    last_sold_unix: i64,
    now_unix: i64,
) -> String {
    if last_sold_unix <= 0 || last_sold_unix > now_unix {
        return t_string!(i18n, analyzer_last_sold_never).to_string();
    }
    let secs = (now_unix - last_sold_unix) as u64;
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    if days > 0 {
        t_string!(i18n, analyzer_last_sold_days_ago).replace("%count%", &days.to_string())
    } else if hours > 0 {
        t_string!(i18n, analyzer_last_sold_hours_ago).replace("%count%", &hours.to_string())
    } else {
        t_string!(i18n, analyzer_last_sold_just_now).to_string()
    }
}

/// Render one cell. `None` for [`CellValue::Custom`]; the host asks the
/// page for those.
pub fn render_cell(
    class: &'static str,
    value: CellValue,
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &CellCtx,
) -> Option<AnyView> {
    Some(match value {
        CellValue::Gil(amount) => view! {
            <div role="cell" class=class><Gil amount=amount /></div>
        }
        .into_any(),
        CellValue::RoiBadge(roi) => view! {
            <div role="cell" class=class>
                <span class=roi_badge_class(roi)>{format!("{roi}%")}</span>
            </div>
        }
        .into_any(),
        CellValue::Count(n) => view! {
            <div role="cell" class=class>{n.to_string()}</div>
        }
        .into_any(),
        CellValue::Confidence(band) => view! {
            <div role="cell" class=class><ConfidenceBadge band=band /></div>
        }
        .into_any(),
        CellValue::LastSoldUnix(unix) => {
            let label = last_sold_label(i18n, unix, ctx.now_unix);
            view! { <div role="cell" class=class>{label}</div> }.into_any()
        }
        CellValue::GilWithPct { amount, pct } => {
            let sub = pct
                .filter(|_| amount > 0)
                .map(|p| format!("{p:+.0}%"))
                .unwrap_or_default();
            view! {
                <div role="cell" class=class>
                    <GilOrDash amount=(amount > 0).then_some(amount) />
                    <div class="text-xs text-[color:var(--color-text-muted)]">{sub}</div>
                </div>
            }
            .into_any()
        }
        CellValue::MutedGil {
            amount,
            pct,
            side,
            capped,
        } => {
            let amount = amount.filter(|a| *a > 0);
            let sub = pct
                .filter(|_| amount.is_some())
                .map(|p| format!("{p:+.0}%"))
                .unwrap_or_default();
            let title = if capped {
                t_string!(i18n, analyzer_alt_cost_capped_title).to_string()
            } else if side == TermRole::Revenue {
                t_string!(i18n, analyzer_alt_revenue_delta_title).to_string()
            } else {
                t_string!(i18n, analyzer_alt_cost_delta_title).to_string()
            };
            view! {
                <div role="cell" class=class title=title>
                    <div class="text-[color:var(--color-text-muted)]">
                        <GilOrDash amount=amount />
                    </div>
                    <div class=SUB_LINE>{sub}</div>
                </div>
            }
            .into_any()
        }
        CellValue::GilWithNote { amount, note } => {
            let (text, note_class) = match note {
                CellNote::None => (String::new(), SUB_LINE.to_string()),
                CellNote::ListingFallback => (
                    t_string!(i18n, analyzer_price_listing_fallback).to_string(),
                    SUB_LINE.to_string(),
                ),
                CellNote::VsMedian { listing, pct } => {
                    let tell =
                        t_string!(i18n, analyzer_price_vs_median, pct = format!("{pct:+.0}%"))
                            .to_string();
                    let text = if listing {
                        format!(
                            "{} · {}",
                            t_string!(i18n, analyzer_price_listing_fallback),
                            tell
                        )
                    } else {
                        tell
                    };
                    (
                        text,
                        format!(
                            "{SUB_LINE_GEOM} {}",
                            signed_delta_class(Some(pct), DELTA_DEAD_BAND_PCT)
                        ),
                    )
                }
                // No `listing ·` prefix here, unlike `VsMedian`. Every
                // locale's fallback word is the same noun this tell already
                // carries, so composing them stutters: "listing · troll
                // listing", "出品 · ぼったくり出品", "Angebot · Troll-Angebot".
                // The warning also already implies the price is not the
                // selected signal, so the prefix earns nothing. Dropping it
                // additionally sidesteps a wording bug: under a `SaleAvg`
                // revenue signal `market_price` is a statistic rather than a
                // listing, and an unclamped mean past 50x its own t-digest
                // median is reachable on a thin week.
                CellNote::Troll { .. } => (
                    t_string!(i18n, analyzer_price_troll).to_string(),
                    format!("{SUB_LINE_GEOM} {SUB_LINE_WARN}"),
                ),
            };
            view! {
                <div role="cell" class=class>
                    <Gil amount=amount />
                    <div class=note_class>{text}</div>
                </div>
            }
            .into_any()
        }
        CellValue::Sparkline(state) => {
            let loading = state.is_loading();
            let (points, pct) = match state {
                Enrich::Ready(v) => (v.points, v.delta_pct.unwrap_or(0.0)),
                _ => (Vec::new(), 0.0),
            };
            view! {
                <div role="cell" class=class>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <span class=if loading { "hidden" } else { "" }>
                        <Sparkline points=points pct_change=pct />
                    </span>
                </div>
            }
            .into_any()
        }
        CellValue::LazyPct(state) => {
            let loading = state.is_loading();
            let pct = match state {
                Enrich::Ready(p) => p,
                _ => None,
            };
            let (text, title) = match (loading, pct) {
                (true, _) => (String::new(), None),
                (false, Some(p)) => (format!("{p:+.0}%"), None),
                (false, None) => (
                    "—".to_string(),
                    Some(t_string!(i18n, analyzer_drift_unavailable).to_string()),
                ),
            };
            let colour = signed_delta_class(pct, DELTA_DEAD_BAND_PCT);
            view! {
                <div role="cell" class=class title=title>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <span class=if loading { "hidden" } else { colour }>{text}</span>
                </div>
            }
            .into_any()
        }
        CellValue::LateCount(state) => {
            let loading = state.is_loading();
            let text = match state {
                Enrich::Ready(n) => n.to_string(),
                Enrich::Missing => "—".to_string(),
                Enrich::Loading => String::new(),
            };
            view! {
                <div role="cell" class=class>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <span class=if loading { "hidden" } else { "" }>{text}</span>
                </div>
            }
            .into_any()
        }
        CellValue::LateGilWithPct(state) => {
            let loading = state.is_loading();
            let (amount, sub) = match state {
                Enrich::Ready((amount, pct)) => (
                    (amount > 0).then_some(amount),
                    pct.filter(|_| amount > 0)
                        .map(|p| format!("{p:+.0}%"))
                        .unwrap_or_default(),
                ),
                _ => (None, String::new()),
            };
            view! {
                <div role="cell" class=class>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <div class=if loading { "hidden" } else { "" }>
                        <GilOrDash amount=amount />
                        <div class="text-xs text-[color:var(--color-text-muted)]">{sub}</div>
                    </div>
                </div>
            }
            .into_any()
        }
        CellValue::Hop { gain, daily_sales } => {
            let (text, has_amount, title) = match gain {
                HopGain::Gain(g) => (
                    signed_gil(g),
                    true,
                    Some(
                        t_string!(
                            i18n,
                            analyzer_hop_gain_title,
                            gil = gil_per_day_label(g as f32 * daily_sales),
                            rate = format!("{daily_sales:.1}")
                        )
                        .to_string(),
                    ),
                ),
                HopGain::Needed => (
                    t_string!(i18n, analyzer_hop_needed).to_string(),
                    false,
                    None,
                ),
                HopGain::Unavailable => ("—".to_string(), false, None),
            };
            // One shape (the `GilOrDash` rule): the icon hides and the value
            // mutes by class; the arms never swap elements.
            view! {
                <div role="cell" class=class title=title>
                    <div class="flex flex-row items-center">
                        <span class=if has_amount { "inline-flex" } else { "hidden" }><GilIcon /></span>
                        <div class=if has_amount { "" } else { "text-[color:var(--color-text-muted)]" }>{text}</div>
                    </div>
                </div>
            }
            .into_any()
        }
        CellValue::SignedGil {
            delta,
            pct,
            unavailable,
        } => {
            let has = delta.is_some();
            let text = delta.map(signed_gil).unwrap_or_else(|| "—".to_string());
            let sub = pct.map(|p| format!("{p:+.0}%")).unwrap_or_default();
            // The `else` arm is defensive, not load-bearing: no caller
            // builds `delta: None` with a `pct`, and for `pct: None`
            // `signed_delta_class` already returns this exact class. It
            // closes the shape for a future caller that separates the two,
            // and a mutation that drops it therefore survives the test
            // below — deliberately, and recorded rather than trusted.
            let value_class = if has {
                signed_delta_class(pct, DELTA_DEAD_BAND_PCT)
            } else {
                "text-[color:var(--color-text-muted)]"
            };
            // Only the "could have had a figure and did not" dash is
            // titled. The "sell scope is your sell world" dash is the whole
            // column at once and the header tooltip is what explains it; a
            // per-cell "Not enough sales" there would be a second wrong
            // answer.
            let title =
                unavailable.then(|| t_string!(i18n, analyzer_drift_unavailable).to_string());
            // One shape (the `GilOrDash` rule): the icon hides and the value
            // mutes by class; the arms never swap elements.
            view! {
                <div role="cell" class=class title=title>
                    <div class="flex flex-row items-center justify-end">
                        <span class=if has { "inline-flex" } else { "hidden" }><GilIcon /></span>
                        <div class=value_class>{text}</div>
                    </div>
                    <div class=SUB_LINE>{sub}</div>
                </div>
            }
            .into_any()
        }
        CellValue::Custom => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos_i18n::context::init_i18n_context;

    fn count(html: &str, needle: &str) -> usize {
        html.matches(needle).count()
    }

    /// Each resource-backed variant keeps one element shape between its
    /// value and no-value states (the `GilOrDash` rule): SSR and CSR must
    /// agree on tags even when a payload lands late.
    #[test]
    fn render_cell_keeps_one_shape_per_variant() {
        // `<Gil>` calls the panicking `use_i18n()`, and building an
        // I18nContext spawns an Effect: stand up both, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 1_700_000_000,
                preview: false,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let a = render_cell(
                "w-32",
                CellValue::GilWithPct {
                    amount: 120,
                    pct: Some(4.2),
                },
                i18n,
                &ctx,
            )
            .unwrap()
            .to_html();
            let b = render_cell(
                "w-32",
                CellValue::GilWithPct {
                    amount: 0,
                    pct: None,
                },
                i18n,
                &ctx,
            )
            .unwrap()
            .to_html();
            assert_eq!(count(&a, "role=\"cell\""), 1);
            assert_eq!(count(&a, "<div"), count(&b, "<div"), "{a}\n{b}");
            assert!(a.contains("+4%"), "{a}");
            assert!(b.contains("—"), "{b}");
            let never = render_cell("w-28", CellValue::LastSoldUnix(0), i18n, &ctx)
                .unwrap()
                .to_html();
            let recent = render_cell("w-28", CellValue::LastSoldUnix(1_699_999_000), i18n, &ctx)
                .unwrap()
                .to_html();
            assert_eq!(count(&never, "<div"), count(&recent, "<div"));
            assert!(render_cell("w-32", CellValue::Custom, i18n, &ctx).is_none());
        });
    }

    #[test]
    fn new_cells_keep_one_shape_per_variant() {
        use crate::analyzer_kit::hop::HopGain;
        use crate::components::term_badge::TermRole;
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 1_700_000_000,
                preview: true,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let render = |v: CellValue| render_cell("w-40", v, i18n, &ctx).unwrap().to_html();
            let a = render(CellValue::MutedGil {
                amount: Some(138),
                pct: Some(38.0),
                side: TermRole::Cost,
                capped: false,
            });
            let b = render(CellValue::MutedGil {
                amount: None,
                pct: None,
                side: TermRole::Cost,
                capped: false,
            });
            let c = render(CellValue::MutedGil {
                amount: None,
                pct: None,
                side: TermRole::Cost,
                capped: true,
            });
            assert_eq!(count(&a, "<div"), count(&b, "<div"), "{a}\n{b}");
            assert_eq!(count(&b, "<div"), count(&c, "<div"));
            assert!(a.contains("+38%"), "{a}");
            assert!(a.contains("title=\"vs the formula's cost input\""), "{a}");
            assert!(b.contains("—"), "{b}");
            assert!(c.contains("Not priced"), "{c}");
            let r = render(CellValue::MutedGil {
                amount: Some(1),
                pct: Some(-4.0),
                side: TermRole::Revenue,
                capped: false,
            });
            assert!(
                r.contains("vs the formula's revenue input") && r.contains("-4%"),
                "{r}"
            );

            let plain = render(CellValue::GilWithNote {
                amount: 120,
                note: CellNote::None,
            });
            let tell = render(CellValue::GilWithNote {
                amount: 120,
                note: CellNote::ListingFallback,
            });
            assert_eq!(count(&plain, "<div"), count(&tell, "<div"));
            assert!(tell.contains(">listing<"), "{tell}");
            assert!(!plain.contains("listing"), "{plain}");

            let gain = render(CellValue::Hop {
                gain: HopGain::Gain(2_150),
                daily_sales: 6.3,
            });
            let loss = render(CellValue::Hop {
                gain: HopGain::Gain(-300),
                daily_sales: 1.0,
            });
            let needed = render(CellValue::Hop {
                gain: HopGain::Needed,
                daily_sales: 6.3,
            });
            let none = render(CellValue::Hop {
                gain: HopGain::Unavailable,
                daily_sales: 6.3,
            });
            for h in [&loss, &needed, &none] {
                assert_eq!(count(&gain, "<div"), count(h, "<div"), "{gain}\n{h}");
                assert_eq!(count(&gain, "<span"), count(h, "<span"));
            }
            assert!(
                gain.contains("+2,150")
                    && gain.contains("title=\"≈ 13.5k gil/day at 6.3 sales/day\""),
                "{gain}"
            );
            assert!(loss.contains("-300") && !loss.contains("+"), "{loss}");
            assert!(
                needed.contains(">needed<") && !needed.contains("title="),
                "{needed}"
            );
            assert!(none.contains("—"), "{none}");
        });
    }

    #[test]
    fn gil_per_day_label_abbreviates() {
        assert_eq!(gil_per_day_label(13_545.0), "13.5k");
        assert_eq!(gil_per_day_label(632.0), "632");
        assert_eq!(gil_per_day_label(-2_150.0), "-2.2k");
        assert_eq!(gil_per_day_label(1_500_000.0), "1.5M");
        assert_eq!(gil_per_day_label(0.0), "0");
    }

    #[test]
    fn last_sold_label_buckets() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let now = 1_700_000_000;
            assert!(!last_sold_label(i18n, 0, now).is_empty());
            let two_days = last_sold_label(i18n, now - 2 * 86_400, now);
            assert!(two_days.contains('2'), "{two_days}");
            let three_hours = last_sold_label(i18n, now - 3 * 3_600, now);
            assert!(three_hours.contains('3'), "{three_hours}");
        });
    }

    #[test]
    fn enrich_maps_the_payload_and_keeps_the_state() {
        assert_eq!(Enrich::Ready(2u8).map(|v| v * 2), Enrich::Ready(4u8));
        assert_eq!(Enrich::<u8>::Missing.map(|v| v * 2), Enrich::Missing);
        assert_eq!(Enrich::<u8>::Loading.map(|v| v * 2), Enrich::Loading);
        assert!(Enrich::<u8>::Loading.is_loading());
        assert!(!Enrich::<u8>::Missing.is_loading());
        assert!(!Enrich::Ready(1u8).is_loading());
    }

    /// Every lazy or late cell renders the same elements in every state:
    /// the skeleton bar and the value slot are both always present and swap
    /// by class. The one exception, and why it is safe: the Trend cell's
    /// `Ready` adds the `<svg>` the `Sparkline` component draws *inside*
    /// its fixed span. `Loading` and `Missing` are shaped alike, and
    /// `Loading` is what the server and the first client paint both render
    /// (the stores are empty on both sides), so hydration never sees
    /// `Ready`.
    #[test]
    fn lazy_cells_keep_one_shape_per_variant() {
        use crate::analyzer_kit::enrichment::SparkValue;
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 1_700_000_000,
                preview: true,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let render = |v: CellValue| render_cell("w-28", v, i18n, &ctx).unwrap().to_html();

            let spark = |e| render(CellValue::Sparkline(e));
            let loading = spark(Enrich::Loading);
            let missing = spark(Enrich::Missing);
            let ready = spark(Enrich::Ready(SparkValue {
                points: vec![100, 110, 120],
                delta_pct: Some(20.0),
            }));
            assert_eq!(count(&loading, "<div"), count(&missing, "<div"));
            assert_eq!(count(&loading, "<span"), count(&missing, "<span"));
            assert_eq!(count(&loading, "role=\"cell\""), 1);
            assert_eq!(count(&ready, "role=\"cell\""), 1);
            assert!(loading.contains("skeleton-shimmer"), "{loading}");
            assert!(!missing.contains("skeleton-shimmer"), "{missing}");
            assert!(ready.contains("<svg"), "{ready}");
            assert!(!loading.contains("<svg") && !missing.contains("<svg"));

            let pct = |e| render(CellValue::LazyPct(e));
            let p_loading = pct(Enrich::Loading);
            let p_missing = pct(Enrich::Missing);
            let p_up = pct(Enrich::Ready(Some(4.0)));
            let p_down = pct(Enrich::Ready(Some(-4.0)));
            let p_flat = pct(Enrich::Ready(Some(0.4)));
            let p_none = pct(Enrich::Ready(None));
            for h in [&p_missing, &p_up, &p_down, &p_flat, &p_none] {
                assert_eq!(
                    count(&p_loading, "<div"),
                    count(h, "<div"),
                    "{p_loading}\n{h}"
                );
                assert_eq!(count(&p_loading, "<span"), count(h, "<span"));
            }
            assert!(
                p_up.contains("+4%") && p_up.contains("text-emerald-300"),
                "{p_up}"
            );
            assert!(
                p_down.contains("-4%") && p_down.contains("text-red-300"),
                "{p_down}"
            );
            assert!(
                p_flat.contains("+0%") && !p_flat.contains("emerald"),
                "{p_flat}"
            );
            // Settled with no percentage reads like no data, with the tell.
            assert!(
                p_none.contains("—") && p_none.contains("Not enough sales"),
                "{p_none}"
            );
            assert!(p_missing.contains("—"), "{p_missing}");
            // Shape parity alone would pass if Loading rendered a value or a
            // dash, so pin the shimmer itself: it is the only thing that
            // separates "still fetching" from "fetched, nothing there".
            assert!(p_loading.contains("skeleton-shimmer"), "{p_loading}");
            assert!(!p_loading.contains("—"), "{p_loading}");
            assert!(!p_missing.contains("skeleton-shimmer"), "{p_missing}");

            let cnt = |e| render(CellValue::LateCount(e));
            let c_loading = cnt(Enrich::Loading);
            let c_missing = cnt(Enrich::Missing);
            let c_ready = cnt(Enrich::Ready(1_234u64));
            for h in [&c_missing, &c_ready] {
                assert_eq!(count(&c_loading, "<div"), count(h, "<div"));
                assert_eq!(count(&c_loading, "<span"), count(h, "<span"));
            }
            assert!(c_ready.contains("1234"), "{c_ready}");
            assert!(c_missing.contains("—"), "{c_missing}");
            assert!(c_loading.contains("skeleton-shimmer"), "{c_loading}");
            assert!(!c_loading.contains("1234"), "{c_loading}");
            assert!(!c_missing.contains("skeleton-shimmer"), "{c_missing}");

            let gil = |e| render(CellValue::LateGilWithPct(e));
            let g_loading = gil(Enrich::Loading);
            let g_missing = gil(Enrich::Missing);
            let g_ready = gil(Enrich::Ready((820, Some(-6.0))));
            let g_zero = gil(Enrich::Ready((0, None)));
            for h in [&g_missing, &g_ready, &g_zero] {
                assert_eq!(
                    count(&g_loading, "<div"),
                    count(h, "<div"),
                    "{g_loading}\n{h}"
                );
                assert_eq!(count(&g_loading, "<span"), count(h, "<span"));
            }
            assert!(
                g_ready.contains("-6%") && g_ready.contains("820"),
                "{g_ready}"
            );
            assert!(g_missing.contains("—") && g_zero.contains("—"));
            // Loading renders `GilOrDash(None)` too — hidden, but the em dash
            // is in the markup — so the assertion above cannot tell the two
            // apart on its own. The shimmer can.
            assert!(g_loading.contains("skeleton-shimmer"), "{g_loading}");
            assert!(!g_missing.contains("skeleton-shimmer"), "{g_missing}");
            assert!(!g_loading.contains("820"), "{g_loading}");
        });
    }

    /// The Price note line keeps Phase D's exact class and text until the
    /// median tell is in it, and the tell's colour composes back to that
    /// same class inside the dead band.
    #[test]
    fn the_price_note_adds_the_median_tell_without_moving_phase_d() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 0,
                preview: true,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let render = |note| {
                render_cell(
                    "w-32",
                    CellValue::GilWithNote { amount: 120, note },
                    i18n,
                    &ctx,
                )
                .unwrap()
                .to_html()
            };
            let plain = render(CellNote::None);
            let listing = render(CellNote::ListingFallback);
            let up = render(CellNote::VsMedian {
                listing: false,
                pct: 4.0,
            });
            let both = render(CellNote::VsMedian {
                listing: true,
                pct: -4.0,
            });
            let flat = render(CellNote::VsMedian {
                listing: false,
                pct: 0.4,
            });
            let troll = render(CellNote::Troll { listing: false });
            let troll_listing = render(CellNote::Troll { listing: true });
            for h in [&listing, &up, &both, &flat, &troll, &troll_listing] {
                assert_eq!(count(&plain, "<div"), count(h, "<div"), "{plain}\n{h}");
            }
            // Phase D's two notes are byte-for-byte what they were.
            let sub = format!("class=\"{SUB_LINE}\"");
            assert!(plain.contains(&sub), "{plain}");
            assert!(
                listing.contains(&sub) && listing.contains(">listing<"),
                "{listing}"
            );
            assert!(
                up.contains("vs median +4%") && up.contains("text-emerald-300"),
                "{up}"
            );
            assert!(
                both.contains("listing · vs median -4%") && both.contains("text-red-300"),
                "{both}"
            );
            // Inside the dead band the composed class IS the plain one.
            assert!(flat.contains(&sub), "{flat}");
            assert_eq!(
                format!(
                    "{SUB_LINE_GEOM} {}",
                    crate::analysis::signed_delta_class(None, crate::analysis::DELTA_DEAD_BAND_PCT)
                ),
                SUB_LINE
            );
            // The troll tell is the warning, in the warning colour — no
            // percentage, and above all no emerald.
            assert!(
                troll.contains("troll listing") && troll.contains(SUB_LINE_WARN),
                "{troll}"
            );
            assert!(
                !troll.contains("emerald") && !troll.contains('%'),
                "{troll}"
            );
            assert!(
                troll_listing.contains("troll listing")
                    && !troll_listing.contains("listing · troll"),
                "{troll_listing}"
            );
            // And that colour is not a new one: it is exactly the class
            // `signed_delta_class` gives a price below its median.
            assert_eq!(
                SUB_LINE_WARN,
                crate::analysis::signed_delta_class(
                    Some(-2.0),
                    crate::analysis::DELTA_DEAD_BAND_PCT
                )
            );
        });
    }

    /// A signed delta keeps one shape across "there is a number", "there is
    /// not" and "there could have been": the gil icon hides by class, the
    /// value mutes by class, and the sub-line element is always present. A
    /// negative delta is the COMMON case for Scope vs home under the
    /// cheapest listing, so this asserts the number survives —
    /// `MutedGil`'s `amount > 0` filter would have swallowed it — and that
    /// a `None` percentage renders muted rather than coloured, which is how
    /// the one-sided listing case avoids a permanent red stripe.
    #[test]
    fn signed_gil_cells_keep_one_shape_and_render_negatives() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 1_700_000_000,
                preview: true,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let render = |v: CellValue| render_cell("w-28", v, i18n, &ctx).unwrap().to_html();
            let down = render(CellValue::SignedGil {
                delta: Some(-1_250),
                pct: Some(-8.0),
                unavailable: false,
            });
            let up = render(CellValue::SignedGil {
                delta: Some(430),
                pct: Some(3.0),
                unavailable: false,
            });
            let one_sided = render(CellValue::SignedGil {
                delta: Some(-1_250),
                pct: None,
                unavailable: false,
            });
            let off = render(CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: false,
            });
            let missing = render(CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: true,
            });
            assert!(down.contains("-1,250"), "{down}");
            assert!(down.contains("text-red-300"), "{down}");
            assert!(down.contains("-8%"), "{down}");
            assert!(up.contains("+430"), "{up}");
            assert!(up.contains("text-emerald-300"), "{up}");
            assert!(
                one_sided.contains("-1,250")
                    && !one_sided.contains("text-red-300")
                    && !one_sided.contains("text-emerald-300"),
                "a dropped percentage must render the delta with no colour: {one_sided}"
            );
            // Colour alone is not enough: `signed_delta_class(None, ..)` is
            // the muted class, so a `pct.unwrap_or(0.0)` slip would render
            // "+0%" on a one-sided cell and still read uncoloured.
            assert!(
                !one_sided.contains('%') && !off.contains('%') && !missing.contains('%'),
                "a dropped percentage leaves the sub-line EMPTY: {one_sided}"
            );
            assert!(off.contains("—"), "{off}");
            assert!(
                !off.contains("title="),
                "the Off dash carries no title: {off}"
            );
            assert!(
                missing.contains("title="),
                "the Unavailable dash is titled: {missing}"
            );
            for html in [&down, &up, &one_sided, &off, &missing] {
                assert_eq!(count(html, "<div"), count(&down, "<div"), "{down}\n{html}");
                assert_eq!(count(html, "<span"), count(&down, "<span"));
            }
        });
    }
}
