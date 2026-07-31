use crate::scene::Color;

/// The shared category palette — same hexes the web UI uses today
/// (`CATEGORY_PALETTE` in price_history_chart.rs).
pub const CATEGORY_PALETTE: [&str; 12] = [
    "#60a5fa", "#f97316", "#34d399", "#a78bfa", "#fb7185", "#facc15", "#22d3ee", "#c084fc",
    "#4ade80", "#f472b6", "#94a3b8", "#fdba74",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// `None` = transparent (web; the page supplies the background).
    pub background: Option<Color>,
    pub text: Color,
    pub text_muted: Color,
    pub grid: Color,
    /// Per-series colors, cycled if there are more series than entries.
    pub palette: Vec<Color>,
    pub volume: Color,
    pub market_average: Color,
    pub trend: Color,
    /// Candle direction pair. Deliberately NOT red/green: the pair separates
    /// on lightness as well as hue so direction survives greyscale and the
    /// common forms of color blindness (see `candle_pair_survives_greyscale`).
    pub candle_up: Color,
    pub candle_down: Color,
    /// Sequential ramp for the density mode, darkest (fewest sales) to
    /// lightest, quantised to 8 steps so cells batch into <= 8 Path nodes.
    pub density_ramp: Vec<Color>,
    pub font_family: String,
}

impl Theme {
    fn base(background: Option<Color>) -> Self {
        Self {
            background,
            text: Color::hex("#e5e7eb"),
            text_muted: Color::hex("#9ca3af"),
            grid: Color::hex("#9ca3af").with_alpha(0.15),
            palette: CATEGORY_PALETTE.iter().map(|c| Color::hex(c)).collect(),
            volume: Color::hex("#22c55e"),
            market_average: Color::hex("#facc15"),
            trend: Color::hex("#94a3b8"),
            candle_up: Color::hex("#5eead4"),
            candle_down: Color::hex("#c2410c"),
            density_ramp: [
                "#1e1b4b", "#312e81", "#3730a3", "#4338ca", "#4f46e5", "#6366f1", "#818cf8",
                "#c7d2fe",
            ]
            .iter()
            .map(|c| Color::hex(c))
            .collect(),
            font_family: "Jaldi, sans-serif".to_string(),
        }
    }

    /// Dark card for PNG output (Discord embeds, the /item/{world}/{id} card).
    pub fn dark_card() -> Self {
        Self::base(Some(Color::hex("#202124")))
    }

    /// Transparent-background variant for the web UI (PR 2).
    pub fn site() -> Self {
        Self::base(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_dark_card_background() {
        let theme = Theme::dark_card();
        assert_eq!(
            theme.background,
            Some(Color::hex("#202124")),
            "Dark card theme should have the specific dark gray background color"
        );
    }

    /// WCAG relative luminance — good enough to prove a greyscale render
    /// keeps the up/down distinction (the spec's colorblind-safety bar).
    fn luminance(c: Color) -> f64 {
        fn lin(u: u8) -> f64 {
            let x = u as f64 / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }

    #[test]
    fn candle_pair_survives_greyscale() {
        let theme = Theme::dark_card();
        let delta = (luminance(theme.candle_up) - luminance(theme.candle_down)).abs();
        assert!(
            delta >= 0.30,
            "candle up/down must differ in luminance by >= 0.30, got {delta:.3}"
        );
    }

    #[test]
    fn density_ramp_holds_lightness_order() {
        let theme = Theme::dark_card();
        assert_eq!(theme.density_ramp.len(), 8, "quantised to 8 opacity steps");
        for pair in theme.density_ramp.windows(2) {
            assert!(
                luminance(pair[0]) < luminance(pair[1]),
                "ramp must be strictly increasing in luminance"
            );
        }
    }

    #[test]
    fn test_theme_site_background() {
        let theme = Theme::site();
        assert_eq!(
            theme.background, None,
            "Site theme should have a transparent (None) background"
        );
    }
}
