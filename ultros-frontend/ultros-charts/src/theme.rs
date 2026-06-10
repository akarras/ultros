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
