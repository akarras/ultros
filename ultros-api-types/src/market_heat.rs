//! Home-page Market Heat band wire types.
//!
//! Five rows per world (Weapons / Tools / Armor / Items / Housing), each
//! carrying a heat band (Hot / Warm / Stable / Cool / NoData) derived
//! from volume-weighted average price change over 24h.
//!
//! The category names come from the FFXIV top-level ItemSearchCategory
//! grouping; the frontend renders friendly i18n'd labels. The bands are
//! chosen by the server so all worlds use consistent thresholds.

use serde::{Deserialize, Serialize};

/// Heat label for a category. Maps directly to color + chip text on the
/// home page. `NoData` means the rollup found no qualifying activity —
/// the row still renders so the band layout stays consistent across
/// worlds, but with a muted chip and no pct.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HeatBand {
    NoData,
    /// Volume-weighted 24h pct change > +5%.
    Hot,
    /// Between +1% and +5%.
    Warm,
    /// Between -1% and +1%.
    Stable,
    /// Below -1%.
    Cool,
}

impl HeatBand {
    pub fn from_pct(pct: f32, item_count: u32) -> Self {
        if item_count == 0 {
            return Self::NoData;
        }
        if pct > 5.0 {
            Self::Hot
        } else if pct > 1.0 {
            Self::Warm
        } else if pct > -1.0 {
            Self::Stable
        } else {
            Self::Cool
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoryHeat {
    /// FFXIV ItemSearchCategory.category top-level grouping
    /// (1=Weapons, 2=Tools, 3=Armor, 4=Items, 5=Housing).
    pub category_id: u8,
    pub item_count: u32,
    pub avg_pct_change_24h: f32,
    pub gil_volume_24h: u64,
    pub band: HeatBand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketHeatResponse {
    pub world_id: i32,
    pub categories: Vec<CategoryHeat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heat_band_from_pct() {
        // NoData when item_count is 0, regardless of pct
        assert_eq!(HeatBand::from_pct(10.0, 0), HeatBand::NoData);
        assert_eq!(HeatBand::from_pct(-10.0, 0), HeatBand::NoData);

        // Hot: pct > 5.0
        assert_eq!(HeatBand::from_pct(5.1, 1), HeatBand::Hot);
        assert_eq!(HeatBand::from_pct(10.0, 1), HeatBand::Hot);

        // Warm: 1.0 < pct <= 5.0
        assert_eq!(HeatBand::from_pct(5.0, 1), HeatBand::Warm);
        assert_eq!(HeatBand::from_pct(1.1, 1), HeatBand::Warm);

        // Stable: -1.0 < pct <= 1.0
        assert_eq!(HeatBand::from_pct(1.0, 1), HeatBand::Stable);
        assert_eq!(HeatBand::from_pct(0.0, 1), HeatBand::Stable);
        assert_eq!(HeatBand::from_pct(-0.9, 1), HeatBand::Stable);

        // Cool: pct <= -1.0
        assert_eq!(HeatBand::from_pct(-1.0, 1), HeatBand::Cool);
        assert_eq!(HeatBand::from_pct(-5.0, 1), HeatBand::Cool);
    }
}
