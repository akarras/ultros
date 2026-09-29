//! Which optional columns a grid shows, as the URL states it.
//!
//! A view names only where it departs from the page defaults —
//! `hide-cols=roi&show-cols=volume_30d` — so a link stays short, and a column
//! a later release turns on by default still appears in it. Older links list
//! every optional column that is on in `cols`; that spelling is still read,
//! and replaced on the next column change.

pub const SHOW_COLS: &str = "show-cols";
pub const HIDE_COLS: &str = "hide-cols";
/// The absolute list older links and saved views carry.
pub const LEGACY_COLS: &str = "cols";
/// Every key that holds column visibility, for writers that replace it.
pub const COLUMN_KEYS: [&str; 3] = [LEGACY_COLS, SHOW_COLS, HIDE_COLS];

/// The column ids in a URL list. `.` is what gets written, because
/// `encodeURIComponent` leaves it alone and the address bar shows it as is;
/// `,` is what older links used and comes out as `%2C`.
pub fn column_ids(raw: &str) -> impl Iterator<Item = &str> {
    raw.split([',', '.'])
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// The column keys of one URL.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnQuery<'a> {
    pub cols: Option<&'a str>,
    pub show: Option<&'a str>,
    pub hide: Option<&'a str>,
}

impl ColumnQuery<'_> {
    /// Whether the URL says anything about columns at all.
    pub fn is_empty(&self) -> bool {
        self.cols.is_none() && self.show.is_none() && self.hide.is_none()
    }

    /// Whether optional column `id` is on, given whether the page shows it by
    /// default. An absolute `cols` list decides alone; otherwise a column
    /// named in both departures stays hidden.
    pub fn visible(&self, id: &str, default: bool) -> bool {
        let names = |raw: Option<&str>| raw.is_some_and(|raw| column_ids(raw).any(|c| c == id));
        if let Some(cols) = self.cols {
            return column_ids(cols).any(|c| c == id);
        }
        if names(self.hide) {
            return false;
        }
        names(self.show) || default
    }
}

/// `(show-cols, hide-cols)` for optional columns given as
/// `(id, default, visible)`: the columns turned on and off relative to their
/// defaults, in the order given. `None` for a side with nothing on it.
pub fn column_departures<'a>(
    columns: impl IntoIterator<Item = (&'a str, bool, bool)>,
) -> (Option<String>, Option<String>) {
    let (mut show, mut hide) = (Vec::new(), Vec::new());
    for (id, default, visible) in columns {
        match (default, visible) {
            (false, true) => show.push(id),
            (true, false) => hide.push(id),
            _ => {}
        }
    }
    let join = |ids: Vec<&str>| (!ids.is_empty()).then(|| ids.join("."));
    (join(show), join(hide))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: [(&str, bool); 4] = [
        ("roi", true),
        ("world", true),
        ("sales_per_day", true),
        ("volume_30d", false),
    ];

    fn visible(query: ColumnQuery) -> Vec<&'static str> {
        COLUMNS
            .iter()
            .filter(|(id, default)| query.visible(id, *default))
            .map(|(id, _)| *id)
            .collect()
    }

    #[test]
    fn no_column_keys_means_the_defaults() {
        let query = ColumnQuery::default();
        assert!(query.is_empty());
        assert_eq!(visible(query), ["roi", "world", "sales_per_day"]);
    }

    #[test]
    fn departures_apply_to_the_defaults() {
        let query = ColumnQuery {
            show: Some("volume_30d"),
            hide: Some("roi.sales_per_day"),
            ..Default::default()
        };
        assert_eq!(visible(query), ["world", "volume_30d"]);
    }

    #[test]
    fn a_legacy_list_is_absolute_in_either_separator() {
        for cols in ["world,volume_30d", "world.volume_30d"] {
            let query = ColumnQuery {
                cols: Some(cols),
                // Ignored: the old list states every column outright.
                show: Some("roi"),
                ..Default::default()
            };
            assert_eq!(visible(query), ["world", "volume_30d"], "{cols}");
        }
        let none = ColumnQuery {
            cols: Some(""),
            ..Default::default()
        };
        assert!(visible(none).is_empty());
    }

    #[test]
    fn departures_round_trip_through_the_reader() {
        let wanted = ["world", "volume_30d"];
        let (show, hide) = column_departures(
            COLUMNS
                .iter()
                .map(|(id, default)| (*id, *default, wanted.contains(id))),
        );
        assert_eq!(show.as_deref(), Some("volume_30d"));
        assert_eq!(hide.as_deref(), Some("roi.sales_per_day"));
        let query = ColumnQuery {
            show: show.as_deref(),
            hide: hide.as_deref(),
            ..Default::default()
        };
        assert_eq!(visible(query), wanted);
        assert_eq!(
            column_departures(COLUMNS.iter().map(|(id, d)| (*id, *d, *d))),
            (None, None)
        );
    }
}
