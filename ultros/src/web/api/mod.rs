pub(crate) mod alerts;
mod best_deals;
mod changelog;
mod cheapest_per_world;
pub(crate) mod discord_lookup;
pub(crate) mod endpoint_validation;
pub(crate) mod endpoints;
mod item_stats;
mod listing_stats;
mod market_heat;
mod market_pulse;
mod movers;
pub(crate) mod push;
mod query;
pub(crate) mod real_time_data;
mod recent_sales;
mod resale_quality;
mod sale_stats;
mod trends;

pub(crate) use best_deals::get_best_deals;
pub(crate) use changelog::get_changelog;
pub(crate) use cheapest_per_world::{CheapestCache, cheapest_per_world, new_cheapest_cache};
pub(crate) use item_stats::get_item_stats;
pub(crate) use listing_stats::get_listing_stats;
pub(crate) use market_heat::get_market_heat;
pub(crate) use market_pulse::get_market_pulse;
pub(crate) use movers::{get_movers, post_sparklines};
pub(crate) use recent_sales::recent_sales;
pub(crate) use resale_quality::post_resale_quality;
pub(crate) use sale_stats::get_sale_stats;
pub(crate) use trends::get_trends;

use std::time::Duration;

use axum::http::HeaderValue;

/// `?format=columnar` selects the struct-of-arrays wire shape the site
/// fetches. Anything else — including no `format` at all — keeps the
/// legacy row-of-objects shape for outside consumers of the public
/// endpoints. Exact, case-sensitive match.
pub(crate) fn is_columnar(format: Option<&str>) -> bool {
    format == Some("columnar")
}

/// Shared `Cache-Control` header for cookie-independent public market GETs.
/// Uses `max-age` + `stale-while-revalidate` + `stale-if-error` without
/// `s-maxage` (Cloudflare documents that `s-maxage` disables
/// `stale-while-revalidate` at the edge).
pub(crate) fn public_cache_control(max_age_secs: u64, swr_secs: u64, sie_secs: u64) -> HeaderValue {
    let header = if swr_secs > 0 {
        format!(
            "public, max-age={max_age_secs}, stale-while-revalidate={swr_secs}, stale-if-error={sie_secs}"
        )
    } else {
        format!("public, max-age={max_age_secs}, stale-if-error={sie_secs}")
    };
    HeaderValue::from_str(&header)
        .expect("ASCII Cache-Control directives are always a valid HeaderValue")
}

/// Derive downstream `Cache-Control` directives from an in-process SWR entry's
/// age and remaining stale lifetime so origin SWR never starts a fresh CDN
/// lifetime for an already-aged or stale generation (RFC 9111 §4.2).
pub(crate) fn public_cache_control_for_age(
    configured_max_age: Duration,
    configured_swr: Duration,
    sie_secs: u64,
    is_stale: bool,
    age: Duration,
    remaining_stale: Duration,
) -> HeaderValue {
    let (max_age_secs, swr_secs) = if is_stale {
        let elapsed_past_fresh = age.saturating_sub(configured_max_age);
        let swr_budget = configured_swr
            .saturating_sub(elapsed_past_fresh)
            .min(remaining_stale);
        (0, swr_budget.as_secs())
    } else {
        let remaining_fresh = configured_max_age.saturating_sub(age);
        (remaining_fresh.as_secs(), configured_swr.as_secs())
    };
    public_cache_control(max_age_secs, swr_secs, sie_secs)
}

/// Default `stale-if-error` window for public market JSON responses so
/// Cloudflare can serve last-good data during brief origin restarts/deploys.
pub(crate) const PUBLIC_STALE_IF_ERROR_SECS: u64 = 600;

#[cfg(test)]
mod tests {
    use super::{PUBLIC_STALE_IF_ERROR_SECS, is_columnar, public_cache_control};

    #[test]
    fn format_query_only_matches_columnar_exactly() {
        assert!(is_columnar(Some("columnar")));
        assert!(!is_columnar(Some("Columnar")));
        assert!(!is_columnar(Some("json")));
        assert!(!is_columnar(Some("")));
        assert!(!is_columnar(None));
    }

    #[test]
    fn public_cache_control_formats_cdn_directives_without_s_maxage() {
        let header = public_cache_control(15, 60, PUBLIC_STALE_IF_ERROR_SECS);
        assert_eq!(
            header,
            "public, max-age=15, stale-while-revalidate=60, stale-if-error=600"
        );
        assert!(!header.to_str().unwrap().contains("s-maxage"));
    }

    #[test]
    fn public_cache_control_for_age_shrinks_freshness_and_caps_stale_windows() {
        use std::time::Duration;

        // Freshly loaded Market Pulse keeps full window.
        assert_eq!(
            super::public_cache_control_for_age(
                Duration::from_secs(30),
                Duration::from_secs(120),
                PUBLIC_STALE_IF_ERROR_SECS,
                false,
                Duration::ZERO,
                Duration::from_secs(300),
            ),
            "public, max-age=30, stale-while-revalidate=120, stale-if-error=600"
        );

        // 10s-old fresh Market Pulse gets remaining 20s max-age.
        assert_eq!(
            super::public_cache_control_for_age(
                Duration::from_secs(30),
                Duration::from_secs(120),
                PUBLIC_STALE_IF_ERROR_SECS,
                false,
                Duration::from_secs(10),
                Duration::from_secs(290),
            ),
            "public, max-age=20, stale-while-revalidate=120, stale-if-error=600"
        );

        // 299s-old stale Market Pulse must never start a new freshness or SWR window.
        assert_eq!(
            super::public_cache_control_for_age(
                Duration::from_secs(30),
                Duration::from_secs(120),
                PUBLIC_STALE_IF_ERROR_SECS,
                true,
                Duration::from_secs(299),
                Duration::from_secs(1),
            ),
            "public, max-age=0, stale-if-error=600"
        );

        // 14s-old stale /cheapest (fresh 5s, stale 15s) gets max-age=0, swr=1.
        assert_eq!(
            super::public_cache_control_for_age(
                Duration::from_secs(15),
                Duration::from_secs(60),
                PUBLIC_STALE_IF_ERROR_SECS,
                true,
                Duration::from_secs(14),
                Duration::from_secs(1),
            ),
            "public, max-age=0, stale-while-revalidate=1, stale-if-error=600"
        );
    }
}
