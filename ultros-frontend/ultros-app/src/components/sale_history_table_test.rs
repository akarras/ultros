#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use ultros_api_types::SaleHistory;

    fn create_sale(date: NaiveDateTime) -> SaleHistory {
        SaleHistory {
            sold_date: date,
            // dummy values for other fields
            quantity: 1,
            price_per_item: 100,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            world_id: 1,
            buyer_name: "Test Buyer".to_string(),
        }
    }

    #[test]
    fn test_find_date_range() {
        let now = Utc::now().naive_utc();
        let hour = Duration::hours(1);

        // Sales are sorted descending (newest first)
        let sales = vec![
            create_sale(now),               // 0: Now
            create_sale(now - hour),        // 1: 1 hour ago
            create_sale(now - hour * 2),    // 2: 2 hours ago
            create_sale(now - hour * 3),    // 3: 3 hours ago
            create_sale(now - hour * 4),    // 4: 4 hours ago
        ];

        // Case 1: Range covers middle part [1h ago, 3h ago]
        // Start: 3 hours ago. End: 1 hour ago.
        let start_date = now - hour * 3;
        let end_date = now - hour;
        let range = start_date..=end_date;

        let result = find_date_range(range, &sales);
        assert!(result.is_some());
        let slice = result.unwrap();

        // Expect indices 1, 2, 3.
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0].sold_date, now - hour);     // Newest in range
        assert_eq!(slice[2].sold_date, now - hour * 3); // Oldest in range

        // Case 2: Range covers everything
        let range_all = (now - hour * 10)..=(now + hour);
        let result_all = find_date_range(range_all, &sales);
        assert!(result_all.is_some());
        assert_eq!(result_all.unwrap().len(), 5);

        // Case 3: Range covers nothing (too old)
        let range_old = (now - hour * 10)..=(now - hour * 6);
        let result_old = find_date_range(range_old, &sales);
        assert!(result_old.is_none());

        // Case 4: Range covers nothing (too new)
        let range_new = (now + hour)..=(now + hour * 2);
        let result_new = find_date_range(range_new, &sales);
        assert!(result_new.is_none());

        // Case 5: Partial overlap (start before, end inside)
        // Start: 5 hours ago. End: 3 hours ago.
        // Range: [5h ago, 3h ago].
        // Sales in range: 3h ago, 4h ago. (Indices 3, 4)
        let range_overlap = (now - hour * 5)..=(now - hour * 3);
        let result_overlap = find_date_range(range_overlap, &sales);
        assert!(result_overlap.is_some());
        let slice_overlap = result_overlap.unwrap();
        assert_eq!(slice_overlap.len(), 2);
        assert_eq!(slice_overlap[0].sold_date, now - hour * 3);
        assert_eq!(slice_overlap[1].sold_date, now - hour * 4);
    }
}
