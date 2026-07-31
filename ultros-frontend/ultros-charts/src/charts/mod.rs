pub mod grid;
pub mod price_density;
pub mod price_history;
#[cfg(test)]
mod snapshot_tests;
pub mod sparkline;

/// Which rendering the price chart uses for its price lane (spec 2 of the
/// chart revamp). `Density` is listed for the toolbar's benefit but is drawn
/// by its own layout (`price_density`), not `price_history` — the
/// price-history layout falls back to `Price` rendering if handed `Density`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartMode {
    #[default]
    Price,
    Candles,
    Range,
    Density,
}

impl ChartMode {
    /// How many series the mode can draw at once; `None` = unlimited.
    /// Series beyond the cap are suppressed from drawing (but stay in the
    /// legend metadata) and the frontend surfaces a hint.
    pub fn series_cap(self) -> Option<usize> {
        match self {
            Self::Price => None,
            Self::Range => Some(2),
            Self::Candles | Self::Density => Some(1),
        }
    }

    /// Stable identifier for keys/debugging; user-facing names come from
    /// the app's i18n layer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Price => "Price",
            Self::Candles => "Candles",
            Self::Range => "Range",
            Self::Density => "Density",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ChartMode;

    #[test]
    fn default_mode_is_price() {
        assert_eq!(ChartMode::default(), ChartMode::Price);
    }

    #[test]
    fn series_caps_follow_the_spec_matrix() {
        assert_eq!(ChartMode::Price.series_cap(), None);
        assert_eq!(ChartMode::Candles.series_cap(), Some(1));
        assert_eq!(ChartMode::Range.series_cap(), Some(2));
        assert_eq!(ChartMode::Density.series_cap(), Some(1));
    }
}
