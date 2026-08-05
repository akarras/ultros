pub mod grid;
pub mod price_density;
pub mod price_history;
#[cfg(test)]
mod snapshot_tests;
pub mod sparkline;

/// One patch boundary prepared for rendering (spec 4). The app builds these
/// from `ultros_api_types::game_history` — layouts only draw. The vec a
/// layout receives must be date-sorted and include the latest patch released
/// *before* the visible window, so the leading stretch is tinted too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MilestoneSpec {
    /// Patch release instant (UTC midnight of the release date).
    pub start: chrono::NaiveDateTime,
    /// `PatchMark` convention: 700 = 7.0, 715 = 7.15.
    pub version: u16,
    /// Expansion index (ARR = 0 …), selecting the band hue.
    pub ex_version: u8,
}

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

/// Wire format for the `?mode=` URL param. Lowercase and stable — part of
/// every shared chart link. Distinct from [`ChartMode::label`], which is a
/// debug/key identifier.
impl std::fmt::Display for ChartMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Price => "price",
            Self::Candles => "candles",
            Self::Range => "range",
            Self::Density => "density",
        })
    }
}

impl std::str::FromStr for ChartMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "price" => Ok(Self::Price),
            "candles" => Ok(Self::Candles),
            "range" => Ok(Self::Range),
            "density" => Ok(Self::Density),
            _ => Err(()),
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

    #[test]
    fn chart_mode_wire_format_round_trips() {
        use std::str::FromStr;
        for mode in [
            ChartMode::Price,
            ChartMode::Candles,
            ChartMode::Range,
            ChartMode::Density,
        ] {
            assert_eq!(ChartMode::from_str(&mode.to_string()), Ok(mode));
        }
        assert_eq!(ChartMode::Price.to_string(), "price");
        assert_eq!(ChartMode::Density.to_string(), "density");
    }

    #[test]
    fn chart_mode_parsing_is_forgiving() {
        use std::str::FromStr;
        assert_eq!(ChartMode::from_str("CANDLES"), Ok(ChartMode::Candles));
        assert_eq!(ChartMode::from_str(" range "), Ok(ChartMode::Range));
        assert_eq!(ChartMode::from_str("nonsense"), Err(()));
    }
}
