//! Visual shell and scope-wide summary around the full market chart engine.
use leptos::prelude::*;
use ultros_api_types::{PriceSeries, floor_history::FloorHistory};

#[derive(Clone, Debug, PartialEq)]
struct MarketSummary {
    latest_floor: Option<f64>,
    floor_change: Option<f64>,
    sale_average: Option<f64>,
    units: i64,
    transactions: u64,
}

fn summarize(sales: Option<&PriceSeries>, floor: Option<&FloorHistory>) -> MarketSummary {
    let mut gil = 0i64;
    let mut units = 0i64;
    let mut transactions = 0u64;
    if let Some(sales) = sales {
        for b in sales.series.iter().flat_map(|s| &s.buckets) {
            gil += b.gil;
            units += b.units;
            transactions += u64::from(b.sales);
        }
    }
    let latest_floor = floor
        .and_then(|f| f.points.last())
        .and_then(|p| p.price)
        .map(f64::from);
    let first_floor = floor
        .and_then(|f| f.points.iter().find_map(|p| p.price))
        .map(f64::from);
    MarketSummary {
        latest_floor,
        floor_change: first_floor
            .zip(latest_floor)
            .filter(|(first, _)| *first > 0.0)
            .map(|(first, last)| (last / first - 1.0) * 100.0),
        sale_average: (units > 0).then(|| gil as f64 / units as f64),
        units,
        transactions,
    }
}
fn number(value: f64) -> String {
    use thousands::Separable;
    (value.round() as i64).separate_with_commas()
}
fn price(value: Option<f64>) -> String {
    value.map(number).unwrap_or_else(|| "—".into())
}
fn interval_label(seconds: i64) -> String {
    if seconds < 3600 {
        format!("{} min", (seconds + 59) / 60)
    } else if seconds < 86400 {
        format!("{:.1} hr", seconds as f64 / 3600.0)
    } else {
        format!("{:.1} days", seconds as f64 / 86400.0)
    }
}

#[component]
pub fn MarketHistory(
    #[prop(into)] sales: Signal<Option<PriceSeries>>,
    #[prop(into)] floor: Signal<Option<FloorHistory>>,
    #[prop(into)] floor_error: Signal<bool>,
    #[prop(into)] scope: Signal<String>,
    children: Children,
) -> impl IntoView {
    let summary = Memo::new(move |_| summarize(sales.get().as_ref(), floor.get().as_ref()));
    view! {
        <style>{include_str!("market_history.css")}</style>
        <section class="market-history" aria-label="Market price history">
            <div class="mh-stats">
                <div class="mh-stat mh-floor-stat"><span>"Last observed floor"</span><strong>{move || price(summary.get().latest_floor)}<small>" gil"</small></strong>
                    <p>{move || summary.get().floor_change.map(|v| format!("{v:+.1}% since first sample")).unwrap_or_else(|| "Waiting for listing history".into())}</p></div>
                <div class="mh-stat"><span>"Average sale price"</span><strong>{move || price(summary.get().sale_average)}<small>" gil"</small></strong><p>"Weighted by units sold"</p></div>
                <div class="mh-stat"><span>"Asking vs. sold"</span><strong>{move || summary.get().latest_floor.zip(summary.get().sale_average).filter(|(_, s)| *s > 0.0).map(|(f, s)| format!("{:+.1}%", (f / s - 1.0) * 100.0)).unwrap_or_else(|| "—".into())}</strong><p>"Last floor vs. period average"</p></div>
                <div class="mh-stat"><span>"Units traded"</span><strong>{move || number(summary.get().units as f64)}</strong><p>{move || format!("{} completed sales", number(summary.get().transactions as f64))}</p></div>
            </div>
            {children()}
            <div class="mh-footnote"><span><i class="mh-floor-dot"></i>"Listings are asking prices, not completed sales."</span><span>{move || format!("{} · selected period and quality · gil per unit", scope.get())}</span></div>
            <p class="mh-data-note" role="status">{move || {
                if floor_error.get() { "Listing history is temporarily unavailable. Sales are still shown.".into() }
                else if let Some(f) = floor.get() {
                    if f.points.is_empty() { "No listing history recorded for this selection yet.".into() }
                    else { format!("Lowest last-observed price across tracked worlds and qualities · sampled every {} · gaps mean no tracked listings. History begins when tracking starts.", interval_label(f.bucket_seconds)) }
                } else { "Loading listing observations…".into() }
            }}</p>
        </section>
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::floor_history::FloorPoint;
    #[test]
    fn empty_latest_listing_is_not_replaced_by_an_older_floor() {
        let f = FloorHistory {
            from: 0,
            to: 10,
            bucket_seconds: 10,
            points: vec![
                FloorPoint {
                    timestamp: 0,
                    price: Some(100),
                },
                FloorPoint {
                    timestamp: 10,
                    price: None,
                },
            ],
        };
        let summary = summarize(None, Some(&f));
        assert_eq!(summary.latest_floor, None);
        assert_eq!(summary.floor_change, None);
    }
    #[test]
    fn combines_sale_totals_including_partial_edge_buckets() {
        use ultros_api_types::{PriceBucket, PriceSeriesEntry, SeriesGroup};
        let ts = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        let bucket = |gil, units| PriceBucket {
            ts,
            open: 1,
            high: 1,
            low: 1,
            close: 1,
            gil,
            units,
            sales: 1,
            p25: 1,
            p50: 1,
            p75: 1,
        };
        let sales = PriceSeries {
            from: ts,
            to: ts,
            bucket_seconds: 60,
            group: SeriesGroup::World,
            series: vec![
                PriceSeriesEntry {
                    id: 1,
                    buckets: vec![bucket(1000, 10)],
                },
                PriceSeriesEntry {
                    id: 2,
                    buckets: vec![bucket(4000, 20)],
                },
            ],
            raw: None,
        };
        let summary = summarize(Some(&sales), None);
        assert_eq!(summary.units, 30);
        assert_eq!(summary.transactions, 2);
        assert_eq!(summary.sale_average, Some(5000.0 / 30.0));
    }
}
