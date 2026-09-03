//! The kit's cell vocabulary: a small value enum rendered by one match,
//! so per-variant markup lives in exactly one place and every
//! resource-backed variant keeps one DOM shape across its states.

use leptos::prelude::*;
use leptos_i18n::I18nContext;
use thousands::Separable;
use ultros_api_types::trends::ConfidenceBand;

use crate::analysis::roi_badge_class;
use crate::components::confidence_badge::ConfidenceBadge;
use crate::components::gil::{Gil, GilIcon, GilOrDash};
use crate::components::term_badge::TermRole;
use crate::i18n::*;

use super::columns::CellCtx;
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
    /// Hop gain / unit: signed gil, the word "needed", or the dash, in one
    /// shape; `daily_sales` feeds the gil/day title.
    Hop {
        gain: HopGain,
        daily_sales: f32,
    },
    /// The page renders this cell itself.
    Custom,
}

/// The sub-line under a [`CellValue::GilWithNote`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CellNote {
    None,
    /// The price fell back to a listing (the selected signal had no row on
    /// the sell world, or the sell world had no listing at all).
    ListingFallback,
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
            let note = match note {
                CellNote::None => String::new(),
                CellNote::ListingFallback => {
                    t_string!(i18n, analyzer_price_listing_fallback).to_string()
                }
            };
            view! {
                <div role="cell" class=class>
                    <Gil amount=amount />
                    <div class=SUB_LINE>{note}</div>
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
}
