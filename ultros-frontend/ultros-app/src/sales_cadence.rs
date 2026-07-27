use crate::analysis::SalesCadence;
use crate::i18n::{I18nKeys, Locale, t_string};
use leptos_i18n::I18nContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalesCadenceTone {
    Success,
    Warning,
    Error,
    Neutral,
}

impl SalesCadenceTone {
    pub fn css_classes(&self) -> &'static str {
        match self {
            Self::Success => "text-emerald-300 border-emerald-400/40 bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]",
            Self::Warning => "text-amber-300 border-amber-400/40 bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]",
            Self::Error => "text-red-300 border-red-400/40 bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]",
            Self::Neutral => "text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalesCadenceLabel {
    Fast,
    Steady,
    Slow,
    NotEnoughData,
}

impl SalesCadenceLabel {
    pub fn get_text(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        match self {
            Self::Fast => t_string!(i18n, sales_cadence_fast).to_string(),
            Self::Steady => t_string!(i18n, sales_cadence_steady).to_string(),
            Self::Slow => t_string!(i18n, sales_cadence_slow).to_string(),
            Self::NotEnoughData => t_string!(i18n, sales_cadence_not_enough_data).to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesCadenceVerdictDisplay {
    pub tone: SalesCadenceTone,
    pub label: SalesCadenceLabel,
    pub velocity_formatted: Option<String>,
}

impl SalesCadenceVerdictDisplay {
    pub fn format_label(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        let label_text = self.label.get_text(i18n);
        if let Some(velocity) = &self.velocity_formatted {
            t_string!(
                i18n,
                sales_cadence_label_with_velocity,
                label = label_text,
                velocity = velocity
            )
            .to_string()
        } else {
            label_text
        }
    }

    /// Short single-line form for tight table cells: just the velocity
    /// ("0.2/day") when known, otherwise the label. The full label belongs
    /// in the badge's `title` so no information is lost.
    pub fn format_compact(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        if let Some(velocity) = &self.velocity_formatted {
            t_string!(i18n, sales_cadence_compact, velocity = velocity).to_string()
        } else {
            self.label.get_text(i18n)
        }
    }
}

pub fn get_sales_cadence_display(
    cadence: SalesCadence,
    sales_per_day: f32,
) -> SalesCadenceVerdictDisplay {
    let (label, tone) = match cadence {
        SalesCadence::Fast => (SalesCadenceLabel::Fast, SalesCadenceTone::Success),
        SalesCadence::Steady => (SalesCadenceLabel::Steady, SalesCadenceTone::Warning),
        SalesCadence::Slow => (SalesCadenceLabel::Slow, SalesCadenceTone::Error),
        SalesCadence::NotEnoughData => {
            (SalesCadenceLabel::NotEnoughData, SalesCadenceTone::Neutral)
        }
    };

    let velocity_formatted = if cadence != SalesCadence::NotEnoughData {
        Some(format!("{:.1}", sales_per_day))
    } else {
        None
    };

    SalesCadenceVerdictDisplay {
        tone,
        label,
        velocity_formatted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::SalesCadence;

    #[test]
    fn test_get_sales_cadence_display() {
        // Fast
        let display = get_sales_cadence_display(SalesCadence::Fast, 10.5);
        assert_eq!(display.label, SalesCadenceLabel::Fast);
        assert_eq!(display.tone, SalesCadenceTone::Success);
        assert_eq!(display.velocity_formatted, Some("10.5".to_string()));

        // Steady
        let display = get_sales_cadence_display(SalesCadence::Steady, 2.0);
        assert_eq!(display.label, SalesCadenceLabel::Steady);
        assert_eq!(display.tone, SalesCadenceTone::Warning);
        assert_eq!(display.velocity_formatted, Some("2.0".to_string()));

        // Slow
        let display = get_sales_cadence_display(SalesCadence::Slow, 0.5);
        assert_eq!(display.label, SalesCadenceLabel::Slow);
        assert_eq!(display.tone, SalesCadenceTone::Error);
        assert_eq!(display.velocity_formatted, Some("0.5".to_string()));

        // NotEnoughData
        let display = get_sales_cadence_display(SalesCadence::NotEnoughData, 0.0);
        assert_eq!(display.label, SalesCadenceLabel::NotEnoughData);
        assert_eq!(display.tone, SalesCadenceTone::Neutral);
        assert_eq!(display.velocity_formatted, None);
    }

    #[test]
    fn test_sales_cadence_tone_css_classes() {
        assert_eq!(
            SalesCadenceTone::Success.css_classes(),
            "text-emerald-300 border-emerald-400/40 bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]"
        );
        assert_eq!(
            SalesCadenceTone::Warning.css_classes(),
            "text-amber-300 border-amber-400/40 bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]"
        );
        assert_eq!(
            SalesCadenceTone::Error.css_classes(),
            "text-red-300 border-red-400/40 bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]"
        );
        assert_eq!(
            SalesCadenceTone::Neutral.css_classes(),
            "text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
        );
    }
}
