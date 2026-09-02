//! The kit's cell vocabulary: a small value enum rendered by one match,
//! so per-variant markup lives in exactly one place and every
//! resource-backed variant keeps one DOM shape across its states.

use leptos::prelude::*;
use leptos_i18n::I18nContext;
use ultros_api_types::trends::ConfidenceBand;

use crate::analysis::roi_badge_class;
use crate::components::confidence_badge::ConfidenceBadge;
use crate::components::gil::{Gil, GilOrDash};
use crate::i18n::*;

use super::columns::CellCtx;

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
    /// The page renders this cell itself.
    Custom,
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
}
