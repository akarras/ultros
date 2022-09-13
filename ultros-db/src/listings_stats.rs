use std::iter::FromIterator;
use crate::entity::active_listing::Model as Listing;


struct ListingStat {

}

struct ListingStats<'a> {
  listings: Vec<(ListingStat, &'a Listing)>
}

impl<'a> ListingStats<'a> {
  fn calculate_stats(listings: &mut [&'a Listing]) -> Self {
    listings.sort_by(|a, b| a.price_per_unit.cmp(&b.price_per_unit).then_with(|| a.quantity.cmp(&b.quantity)));
    let total_items_listed : i32 = listings.iter().map(|m| m.quantity).sum();
    let average_price : i64 = listings.iter().map(|m| m.quantity * m.price_per_unit).sum();
    Self {
        listings: Vec::new(),
    }
  }
}

impl<'a> FromIterator<&'a Listing> for ListingStats<'a> {
    fn from_iter<T: IntoIterator<Item = &'a Listing>>(iter: T) -> Self {
        Self {
            listings: Vec::new(),
        }
    }
}

