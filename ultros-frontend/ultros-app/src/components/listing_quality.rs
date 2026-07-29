use ultros_api_types::ActiveListing;

/// Which quality of listing the reader is currently looking at.
///
/// `All` is the default so the merged table opens showing exactly the rows the
/// two split HQ/NQ tables used to show between them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ListingQuality {
    #[default]
    All,
    Hq,
    Nq,
}

impl ListingQuality {
    /// True when a listing carrying this `hq` flag belongs in the current view.
    pub(crate) fn matches(self, hq: bool) -> bool {
        match self {
            ListingQuality::All => true,
            ListingQuality::Hq => hq,
            ListingQuality::Nq => !hq,
        }
    }
}

/// Keep only the rows matching `quality`. Input order is preserved, so a
/// caller that has already sorted by price stays sorted.
pub(crate) fn filter_by_quality<T>(
    listings: Vec<(ActiveListing, T)>,
    quality: ListingQuality,
) -> Vec<(ActiveListing, T)> {
    if quality == ListingQuality::All {
        return listings;
    }
    listings
        .into_iter()
        .filter(|(listing, _)| quality.matches(listing.hq))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use ultros_api_types::ActiveListing;

    fn listing(id: i32, hq: bool) -> (ActiveListing, ()) {
        (
            ActiveListing {
                id,
                world_id: 100,
                item_id: 1,
                retainer_id: id,
                price_per_unit: id * 10,
                quantity: 1,
                hq,
                timestamp: NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            },
            (),
        )
    }

    #[test]
    fn all_is_the_default() {
        assert_eq!(ListingQuality::default(), ListingQuality::All);
    }

    #[test]
    fn all_keeps_every_row() {
        let rows = vec![listing(1, true), listing(2, false)];

        let result = filter_by_quality(rows.clone(), ListingQuality::All);

        assert_eq!(result, rows);
    }

    #[test]
    fn hq_keeps_only_high_quality() {
        let rows = vec![listing(1, true), listing(2, false), listing(3, true)];

        let result = filter_by_quality(rows, ListingQuality::Hq);

        assert_eq!(
            result.iter().map(|(l, _)| l.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn nq_keeps_only_normal_quality() {
        let rows = vec![listing(1, true), listing(2, false), listing(3, true)];

        let result = filter_by_quality(rows, ListingQuality::Nq);

        assert_eq!(
            result.iter().map(|(l, _)| l.id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn filtering_preserves_input_order() {
        let rows = vec![listing(5, false), listing(1, false), listing(3, false)];

        let result = filter_by_quality(rows, ListingQuality::Nq);

        assert_eq!(
            result.iter().map(|(l, _)| l.id).collect::<Vec<_>>(),
            vec![5, 1, 3]
        );
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let rows: Vec<(ActiveListing, ())> = Vec::new();

        assert!(filter_by_quality(rows, ListingQuality::Hq).is_empty());
    }
}
