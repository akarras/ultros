// Keep fixtures readable as rows, then mirror the API's opt-in columnar wire
// contract at the interception boundary. Non-market and error bodies pass through.
function marketWireBody(url, body) {
  if (typeof url === 'string') url = new URL(url, 'http://localhost');
  if (url.searchParams.get('format') !== 'columnar' || body.error) return body;
  const kind = url.pathname.split('/')[3];
  const columns = (rows, defaults) => Object.fromEntries(Object.entries(defaults)
    .map(([key, fallback]) => [key, rows.map(row => row[key] ?? fallback)]));
  if (kind === 'cheapest') return {
    ...columns(body.cheapest_listings, { item_id: 0, hq: false, world_id: 0 }),
    price: body.cheapest_listings.map(row => row.cheapest_price),
  };
  if (kind === 'recentSales') return {
    ...columns(body.sales, { item_id: 0, hq: false }),
    count: body.sales.map(row => row.sales.length),
    price: body.sales.flatMap(row => row.sales.map(sale => sale.price_per_unit)),
    sold_unix: body.sales.flatMap(row => row.sales.map(sale => {
      // Rust's NaiveDateTime wire text has no suffix, but represents UTC.
      const date = /(?:Z|[+-]\d\d:\d\d)$/.test(sale.sale_date) ? sale.sale_date : `${sale.sale_date}Z`;
      return Math.floor(Date.parse(date) / 1000);
    })),
  };
  if (kind === 'sale_stats') return columns(body.stats, {
    item_id: 0, hq: false, min_price: 0, median_price: 0, avg_price: 0, num_sold: 0,
    last_sold_unix: 0, units_sold: 0, vwap: 0, gil_volume: 0, sales_per_day: 0, confidence: 'unknown',
  });
  if (kind === 'listing_stats') {
    const result = columns(body.stats, {
      item_id: 0, hq: false, alive_count: 0, alive_units: 0, distinct_retainers: 0,
      oldest_reviewed_unix: 0, median_age_secs: 0, floor_alive: 0,
    });
    if (body.stats.some(row => row.window)) result.window = body.stats.map(row => row.window ?? emptyListingWindow());
    return result;
  }
  return body;
}

// Matches ListingWindowStats::default() for rows with no window in a mixed body.
function emptyListingWindow() {
  const coverage = () => ({ first_observed_unix: null, last_observed_unix: null, observed_span_secs: 0, continuity_verified: false });
  return {
    window_days: 0, from: 0, to: 0, additions: 0, removals: 0, listing_coverage: coverage(),
    floor_min: null, floor_max: null, floor_known_secs: 0, floor_empty_secs: 0, floor_unknown_secs: 0,
    matches: { matched: 0, ambiguous: 0, repriced: 0, unmatched: 0, sales_without_receipt: 0,
      receipt_coverage: coverage(), received_sales: 0, settled_through_unix: 0, pending: 0,
      median_time_to_sell_secs: null, age_origin: 'last_review_time' },
    stock_status: 'unavailable', days_of_stock: null, undercuts: 0, undercuts_per_day: null, undercut_median: null,
  };
}

module.exports = { marketWireBody };
