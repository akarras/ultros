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

pub(crate) use best_deals::{BestDealsQuery, get_best_deals, serve_best_deals};
pub(crate) use changelog::get_changelog;
pub(crate) use cheapest_per_world::{
    CheapestCache, cheapest_per_world, new_cheapest_cache, serve_cheapest_per_world,
};
pub(crate) use item_stats::get_item_stats;
pub(crate) use listing_stats::{get_listing_stats, serve_listing_stats};
pub(crate) use market_heat::get_market_heat;
pub(crate) use market_pulse::{get_market_pulse, serve_market_pulse};
pub(crate) use movers::{get_movers, post_sparklines, serve_movers};
pub(crate) use recent_sales::{recent_sales, serve_recent_sales};
pub(crate) use resale_quality::post_resale_quality;
pub(crate) use sale_stats::{get_sale_stats, serve_sale_stats};
pub(crate) use trends::{TrendsQuery, get_trends, serve_trends};

use std::time::{Duration, Instant};

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
/// origin timestamps (`stored_at`, `fresh_until`, `stale_until`) evaluated at
/// send time `now` (after encoding finishes), so neither origin SWR nor slow
/// compression can ever extend downstream freshness or SWR past the origin
/// deadline (RFC 9111 §4.2).
pub(crate) fn public_cache_control_for_deadlines(
    configured_max_age: Duration,
    configured_swr: Duration,
    sie_secs: u64,
    stored_at: Instant,
    fresh_until: Instant,
    stale_until: Instant,
    now: Instant,
) -> HeaderValue {
    if now >= stale_until {
        return public_cache_control(0, 0, sie_secs);
    }
    let elapsed = now.saturating_duration_since(stored_at);
    let remaining_stale = stale_until.saturating_duration_since(now);
    if now >= fresh_until {
        let elapsed_past_fresh = elapsed.saturating_sub(configured_max_age);
        let swr_budget = configured_swr
            .saturating_sub(elapsed_past_fresh)
            .min(remaining_stale);
        return public_cache_control(0, swr_budget.as_secs(), sie_secs);
    }
    let remaining_fresh = fresh_until
        .saturating_duration_since(now)
        .min(configured_max_age.saturating_sub(elapsed))
        .min(remaining_stale);
    let max_age_secs = remaining_fresh.as_secs();
    let remaining_after_fresh = remaining_stale.saturating_sub(Duration::from_secs(max_age_secs));
    let swr_secs = configured_swr.min(remaining_after_fresh).as_secs();
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
    fn public_cache_control_for_deadlines_shrinks_freshness_and_caps_expired_windows() {
        use std::time::{Duration, Instant};

        let t0 = Instant::now();

        // Freshly loaded Market Pulse keeps full window.
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(30),
                Duration::from_secs(120),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(30),
                t0 + Duration::from_secs(300),
                t0,
            ),
            "public, max-age=30, stale-while-revalidate=120, stale-if-error=600"
        );

        // 10s-old fresh Market Pulse gets remaining 20s max-age.
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(30),
                Duration::from_secs(120),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(30),
                t0 + Duration::from_secs(300),
                t0 + Duration::from_secs(10),
            ),
            "public, max-age=20, stale-while-revalidate=120, stale-if-error=600"
        );

        // 299s-old stale Market Pulse must never start a new freshness or SWR window.
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(30),
                Duration::from_secs(120),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(30),
                t0 + Duration::from_secs(300),
                t0 + Duration::from_secs(299),
            ),
            "public, max-age=0, stale-if-error=600"
        );

        // Freshly loaded /cheapest (fresh 5s, stale 15s) caps max-age to 5s
        // and combined max-age + SWR to the 15s origin stale cutoff.
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(15),
                Duration::from_secs(60),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(5),
                t0 + Duration::from_secs(15),
                t0,
            ),
            "public, max-age=5, stale-while-revalidate=10, stale-if-error=600"
        );

        // /cheapest at age 4.9s has 0.1s fresh and 10.1s stale remaining:
        // must floor before converting to whole seconds so it never overshoots
        // origin freshness (`max-age=0, stale-while-revalidate=10`).
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(15),
                Duration::from_secs(60),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(5),
                t0 + Duration::from_secs(15),
                t0 + Duration::from_millis(4_900),
            ),
            "public, max-age=0, stale-while-revalidate=10, stale-if-error=600"
        );

        // 14s-old stale /cheapest (fresh 5s, stale 15s) gets max-age=0, swr=1.
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(15),
                Duration::from_secs(60),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(5),
                t0 + Duration::from_secs(15),
                t0 + Duration::from_secs(14),
            ),
            "public, max-age=0, stale-while-revalidate=1, stale-if-error=600"
        );

        // Any entry whose send time `now` reaches/passes `stale_until` (even
        // if it was classified Fresh when fetched before encoding) gets 0/0.
        assert_eq!(
            super::public_cache_control_for_deadlines(
                Duration::from_secs(15),
                Duration::from_secs(60),
                PUBLIC_STALE_IF_ERROR_SECS,
                t0,
                t0 + Duration::from_secs(5),
                t0 + Duration::from_secs(15),
                t0 + Duration::from_secs(16),
            ),
            "public, max-age=0, stale-if-error=600"
        );
    }
}
