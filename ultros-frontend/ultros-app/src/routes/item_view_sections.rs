//! Jump-nav destinations for the item view.
//!
//! The order here is the page's DOM order. Later lens work reorders the
//! rendered sections with CSS `order` while leaving this DOM order — and
//! therefore this list — untouched.

/// One navigable section of `/item/:world/:id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,
    Listings,
    History,
    Sources,
    Related,
}

impl Section {
    /// Every section, in DOM order.
    pub const ALL: [Section; 5] = [
        Section::Overview,
        Section::Listings,
        Section::History,
        Section::Sources,
        Section::Related,
    ];

    /// The `id` attribute the section renders with, and the fragment the nav
    /// links to.
    ///
    /// `listings` and `history` predate this module — `MarketStatsPanel`'s
    /// stat tiles and savings callout link to them directly — so they are
    /// fixed by compatibility, not choice.
    pub fn id(self) -> &'static str {
        match self {
            Section::Overview => "overview",
            Section::Listings => "listings",
            Section::History => "history",
            Section::Sources => "sources",
            Section::Related => "related",
        }
    }

    /// Fragment link to this section.
    pub fn href(self) -> String {
        format!("#{}", self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_section_has_a_unique_id() {
        let ids: HashSet<&str> = Section::ALL.iter().map(|s| s.id()).collect();

        assert_eq!(ids.len(), Section::ALL.len());
    }

    #[test]
    fn no_id_is_empty() {
        assert!(Section::ALL.iter().all(|s| !s.id().is_empty()));
    }

    #[test]
    fn preexisting_anchors_are_preserved() {
        // MarketStatsPanel already links to these; renaming them would break
        // the savings callout and the stat-tile links.
        assert_eq!(Section::Listings.id(), "listings");
        assert_eq!(Section::History.id(), "history");
    }

    #[test]
    fn overview_is_first() {
        assert_eq!(Section::ALL.first(), Some(&Section::Overview));
    }

    #[test]
    fn href_is_a_fragment_link() {
        assert_eq!(Section::Listings.href(), "#listings");
    }
}
