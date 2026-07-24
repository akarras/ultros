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
}
