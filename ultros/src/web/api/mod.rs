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
    HeaderValue::from_str(&format!(
        "public, max-age={max_age_secs}, stale-while-revalidate={swr_secs}, stale-if-error={sie_secs}"
    ))
    .expect("ASCII Cache-Control directives are always a valid HeaderValue")
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
}
