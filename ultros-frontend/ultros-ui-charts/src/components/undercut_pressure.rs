//! Item page undercut pressure: the pane under the price chart and the four
//! stat cards. World scope only — the route gates the fetch.
use crate::i18n::{t, t_string, use_i18n};
use leptos::prelude::*;
use ultros_api_types::undercut_pressure::{PressureBucket, UndercutPressure, WarStatus};
use ultros_charts::charts::undercut_pressure::{
    PressurePalette, PressurePaneOptions, build_undercut_pressure_chart, pane_height,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurationUnit {
    Minutes,
    Hours,
    Days,
}

pub(crate) fn duration_parts(secs: i64) -> (DurationUnit, i64) {
    if secs < 90 * 60 {
        (DurationUnit::Minutes, ((secs + 30) / 60).max(1))
    } else if secs < 36 * 3600 {
        (DurationUnit::Hours, (secs + 1800) / 3600)
    } else {
        (DurationUnit::Days, (secs + 43_200) / 86_400)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContestedLevel {
    Always,
    Often,
    Quiet,
}

pub(crate) fn contested_level(share: f64) -> ContestedLevel {
    if share >= 0.8 {
        ContestedLevel::Always
    } else if share >= 0.3 {
        ContestedLevel::Often
    } else {
        ContestedLevel::Quiet
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrendWord {
    Falling,
    Steady,
    Rising,
}

pub(crate) fn trend_word(fraction: f64) -> TrendWord {
    if fraction < -0.02 {
        TrendWord::Falling
    } else if fraction > 0.02 {
        TrendWord::Rising
    } else {
        TrendWord::Steady
    }
}

/// Rounded (left, undercut) percentages that always sum to 100.
pub(crate) fn outcome_split(left: u32, undercut: u32) -> Option<(u32, u32)> {
    let total = left + undercut;
    (total > 0).then(|| {
        let left_pct = (f64::from(left) / f64::from(total) * 100.0).round() as u32;
        (left_pct, 100 - left_pct)
    })
}

pub fn pressure_bucket_at(p: &UndercutPressure, ts: i64) -> Option<&PressureBucket> {
    p.buckets
        .iter()
        .find(|b| ts >= b.start && ts < b.start + p.bucket_seconds)
}

fn percent(fraction: f64) -> String {
    format!("{:+.1}%", fraction * 100.0)
}

#[component]
pub fn UndercutPressureCards(
    #[prop(into)] pressure: Signal<Option<UndercutPressure>>,
    #[prop(into)] error: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let summary = Memo::new(move |_| pressure.get().map(|p| p.summary));
    let duration = move |secs: i64| match duration_parts(secs) {
        (DurationUnit::Minutes, n) => t_string!(i18n, undercut_pressure_minutes, n = n).to_string(),
        (DurationUnit::Hours, n) => t_string!(i18n, undercut_pressure_hours, n = n).to_string(),
        (DurationUnit::Days, n) => t_string!(i18n, undercut_pressure_days, n = n).to_string(),
    };
    let war_value = move || match summary.get().map(|s| s.war) {
        Some(WarStatus::Active) => t_string!(i18n, undercut_pressure_war_active).to_string(),
        Some(WarStatus::Ended { at }) => {
            let hours = ((chrono::Utc::now().timestamp() - at).max(0) + 1800) / 3600;
            t_string!(i18n, undercut_pressure_war_ended, hours = hours).to_string()
        }
        _ => t_string!(i18n, undercut_pressure_war_none).to_string(),
    };
    let war_detail = move || {
        let s = summary.get()?;
        if s.war != WarStatus::None
            && let Some(w) = s.last_war.as_ref()
        {
            let change = w.floor_change.map(percent).unwrap_or_else(|| "—".into());
            return Some(
                t_string!(
                    i18n,
                    undercut_pressure_war_detail,
                    sellers = w.sellers,
                    change = change
                )
                .to_string(),
            );
        }
        let level = match contested_level(s.contested_share?) {
            ContestedLevel::Always => {
                t_string!(i18n, undercut_pressure_contested_always).to_string()
            }
            ContestedLevel::Often => t_string!(i18n, undercut_pressure_contested_often).to_string(),
            ContestedLevel::Quiet => t_string!(i18n, undercut_pressure_contested_quiet).to_string(),
        };
        Some(match s.typical_undercuts_per_hour {
            Some(rate) => {
                let rate = t_string!(
                    i18n,
                    undercut_pressure_typical_rate,
                    rate = format!("{rate:.1}")
                )
                .to_string();
                format!("{level} · {rate}")
            }
            None => level,
        })
    };
    let trend_detail = move || {
        summary
            .get()
            .and_then(|s| s.floor_trend_24h)
            .map(|f| match trend_word(f) {
                TrendWord::Falling => t_string!(i18n, undercut_pressure_trend_falling).to_string(),
                TrendWord::Steady => t_string!(i18n, undercut_pressure_trend_steady).to_string(),
                TrendWord::Rising => t_string!(i18n, undercut_pressure_trend_rising).to_string(),
            })
    };
    view! {
        <style>{include_str!("undercut_pressure.css")}</style>
        <Show when=move || error.get()>
            <p class="mh-pressure-unavailable" role="status">
                {t!(i18n, undercut_pressure_unavailable)}
            </p>
        </Show>
        <Show when=move || summary.with(Option::is_some)>
            <div class="mh-stats mh-pressure-stats">
                <div class="mh-stat">
                    <span>{t!(i18n, undercut_pressure_card_trend)}</span>
                    <strong>
                        {move || {
                            summary
                                .get()
                                .and_then(|s| s.floor_trend_24h)
                                .map(percent)
                                .unwrap_or_else(|| "—".into())
                        }}
                    </strong>
                    <p>{trend_detail}</p>
                </div>
                <div
                    class="mh-stat"
                    class:mh-pressure-war=move || {
                        summary.get().is_some_and(|s| s.war == WarStatus::Active)
                    }
                >
                    <span>{t!(i18n, undercut_pressure_card_war)}</span>
                    <strong>{war_value}</strong>
                    <p>{war_detail}</p>
                </div>
                <div class="mh-stat">
                    <span>{t!(i18n, undercut_pressure_card_holds)}</span>
                    <strong>
                        {move || {
                            summary
                                .get()
                                .and_then(|s| s.floor_holds_median_secs)
                                .map(duration)
                                .unwrap_or_else(|| "—".into())
                        }}
                    </strong>
                    <p>{t!(i18n, undercut_pressure_holds_detail)}</p>
                </div>
                <div class="mh-stat">
                    <span>{t!(i18n, undercut_pressure_card_outcome)}</span>
                    <strong>
                        {move || {
                            summary
                                .get()
                                .and_then(|s| outcome_split(s.episodes_left, s.episodes_undercut))
                                .map(|(l, u)| format!("{l}% / {u}%"))
                                .unwrap_or_else(|| "—".into())
                        }}
                    </strong>
                    <p>{t!(i18n, undercut_pressure_outcome_detail)}</p>
                </div>
            </div>
        </Show>
    }
}

#[component]
pub fn UndercutPressurePane(
    #[prop(into)] pressure: Signal<Option<UndercutPressure>>,
    /// (bucket start, sale rows) summed over the price series.
    #[prop(into)]
    sales: Signal<Vec<(i64, u32)>>,
    #[prop(into)] time_domain: Signal<Option<(i64, i64)>>,
    #[prop(into)] width: Signal<f32>,
    /// Crosshair x in scene units (the price chart's hovered bucket).
    #[prop(into)]
    hover_x: Signal<Option<f32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let model = Memo::new(move |_| {
        let p = pressure.get()?;
        let domain = time_domain.get()?;
        let width = width.get();
        Some(build_undercut_pressure_chart(
            &p,
            &sales.get(),
            &PressurePaneOptions {
                width,
                height: pane_height(width),
                time_domain: domain,
                palette: PressurePalette::default(),
            },
        ))
    });
    move || {
        let m = model.get()?;
        let (top, bottom) = (m.bars_top, m.bars_bottom);
        Some(view! {
            <div class="mh-pressure-pane">
                <div class="mh-pressure-legend">
                    <span class="mh-pressure-title">{t!(i18n, undercut_pressure_title)}</span>
                    <span>
                        <i class="mh-swatch mh-swatch-cut"></i>
                        {t!(i18n, undercut_pressure_legend_cuts)}
                    </span>
                    <span>
                        <i class="mh-swatch mh-swatch-trim"></i>
                        {t!(i18n, undercut_pressure_legend_trims)}
                    </span>
                    <span>
                        <i class="mh-swatch mh-swatch-sales"></i>
                        {t!(i18n, undercut_pressure_legend_sales)}
                    </span>
                    <span>
                        <i class="mh-swatch mh-swatch-baseline"></i>
                        {t!(i18n, undercut_pressure_legend_baseline)}
                    </span>
                    <span>
                        <i class="mh-swatch mh-swatch-war"></i>
                        {t!(i18n, undercut_pressure_state_war)}
                    </span>
                    <span>
                        <i class="mh-swatch mh-swatch-churn"></i>
                        {t!(i18n, undercut_pressure_state_churn)}
                    </span>
                    <span>
                        <i class="mh-swatch mh-swatch-calm"></i>
                        {t!(i18n, undercut_pressure_state_calm)}
                    </span>
                </div>
                <svg
                    class="block w-full h-auto"
                    role="img"
                    aria-label=move || t_string!(i18n, undercut_pressure_pane_label).to_string()
                    viewBox=format!("0 0 {:.0} {:.0}", m.scene.width, m.scene.height)
                    preserveAspectRatio="xMidYMid meet"
                >
                    {crate::components::price_history_chart::pressure_scene_view(&m.scene)}
                    {move || {
                        hover_x
                            .get()
                            .map(|x| {
                                view! {
                                    <line
                                        x1=x
                                        x2=x
                                        y1=top
                                        y2=bottom
                                        class="mh-pressure-crosshair"
                                    />
                                }
                            })
                    }}
                </svg>
            </div>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_humanize_by_magnitude() {
        assert_eq!(duration_parts(90), (DurationUnit::Minutes, 2));
        assert_eq!(duration_parts(35 * 60), (DurationUnit::Minutes, 35));
        assert_eq!(duration_parts(2 * 3600 + 1000), (DurationUnit::Hours, 2));
        assert_eq!(duration_parts(3 * 86_400), (DurationUnit::Days, 3));
    }

    #[test]
    fn contested_and_trend_thresholds() {
        assert_eq!(contested_level(0.8), ContestedLevel::Always);
        assert_eq!(contested_level(0.3), ContestedLevel::Often);
        assert_eq!(contested_level(0.29), ContestedLevel::Quiet);
        assert_eq!(trend_word(-0.021), TrendWord::Falling);
        assert_eq!(trend_word(0.02), TrendWord::Steady);
        assert_eq!(trend_word(0.021), TrendWord::Rising);
    }

    #[test]
    fn outcome_split_sums_to_100() {
        assert_eq!(outcome_split(0, 0), None);
        assert_eq!(outcome_split(2, 1), Some((67, 33)));
        assert_eq!(outcome_split(5, 3), Some((63, 37)));
    }

    #[test]
    fn bucket_lookup_contains_the_timestamp() {
        let p = ultros_charts::charts::undercut_pressure::tests_fixture();
        assert_eq!(
            pressure_bucket_at(&p, 2 * 3600 + 5).map(|b| b.start),
            Some(2 * 3600)
        );
        assert!(pressure_bucket_at(&p, 99 * 3600).is_none());
    }
}
