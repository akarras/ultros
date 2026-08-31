//! Minimal Lodestone character-profile lookup.
//!
//! Claiming a character needs exactly two facts about it — its name and its
//! home world — but `lodestone::model::profile::Profile` parses the *entire*
//! profile page into typed enums (race, clan, gender, attributes, every class
//! level) and fails the whole request if any single one of them is unknown.
//!
//! Those enums predate Shadowbringers: `Race` knows only the 1.0/2.0 six, so
//! `Race::from_str("VIERA")` is an error and **every claim of a Viera or
//! Hrothgar character 500'd** (GlitchTip #7286,
//! `CharacterClaimError(Lodestone(SearchError(RaceParseError("VIERA"))))`).
//! `Server` has the same shape of problem every time Square adds a world.
//!
//! Reading the two fields we actually use straight off the page sidesteps all
//! of it, and skips the second (`class_job`) page fetch `Profile::get_async`
//! does for class levels nothing here looks at. Character *search*
//! (`lodestone::search`) is unaffected — it already returns name and world as
//! plain strings — so it keeps using the crate.

use select::document::Document;
use select::predicate::Class;
use thiserror::Error;

static BASE_PROFILE_URL: &str = "https://na.finalfantasyxiv.com/lodestone/character/";

/// The name and home world shown on a Lodestone character page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CharacterSummary {
    pub(crate) name: String,
    pub(crate) home_world: String,
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("Lodestone has no character with id {0}")]
    CharacterNotFound(u32),
    #[error("Error fetching lodestone profile: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Lodestone profile {id} is missing its {field}")]
    MissingField { id: u32, field: &'static str },
}

/// Fetches character `character_id`'s name and home world from the Lodestone.
pub(crate) async fn get_character_summary(
    client: &reqwest::Client,
    character_id: u32,
) -> Result<CharacterSummary, ProfileError> {
    let response = client
        .get(format!("{BASE_PROFILE_URL}{character_id}/"))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ProfileError::CharacterNotFound(character_id));
    }
    let html = response.error_for_status()?.text().await?;
    parse_character_summary(character_id, &html)
}

fn parse_character_summary(
    character_id: u32,
    html: &str,
) -> Result<CharacterSummary, ProfileError> {
    let document = Document::from(html);
    let text = |class, field| {
        let value = document
            .find(Class(class))
            .next()
            .map(|node| node.text())
            .ok_or(ProfileError::MissingField {
                id: character_id,
                field,
            })?;
        Ok::<_, ProfileError>(value)
    };

    let name = text("frame__chara__name", "name")?.trim().to_string();
    if name.is_empty() {
        return Err(ProfileError::MissingField {
            id: character_id,
            field: "name",
        });
    }

    // The world line reads `Brynhildr [Crystal]`, and on some pages separates
    // the world from its data center with a non-breaking space instead of a
    // plain one. Either way the world name is the first token.
    let home_world = text("frame__chara__world", "home world")?
        .split([' ', '\u{A0}'])
        .find(|token| !token.is_empty())
        .unwrap_or_default()
        .to_string();
    if home_world.is_empty() {
        return Err(ProfileError::MissingField {
            id: character_id,
            field: "home world",
        });
    }

    Ok(CharacterSummary { name, home_world })
}

#[cfg(test)]
mod test {
    use super::*;

    /// Trimmed from the live page for character 32011760 — the Viera whose
    /// claim attempts are GlitchTip #7286.
    const VIERA_PROFILE: &str = include_str!("../test_data/lodestone_viera_profile.html");

    #[test]
    fn reads_name_and_home_world_of_a_viera() {
        // The race/clan line (`Viera`, `Veena`) is exactly what the `lodestone`
        // crate's `Profile` parser rejects; nothing here looks at it.
        let summary = parse_character_summary(32011760, VIERA_PROFILE).unwrap();
        assert_eq!(
            summary,
            CharacterSummary {
                name: "Kalanne Ymir".to_string(),
                home_world: "Brynhildr".to_string(),
            }
        );
    }

    #[test]
    fn splits_a_non_breaking_space_between_world_and_data_center() {
        let html = r#"<p class="frame__chara__name">Heart Mocha</p>
             <p class="frame__chara__world">Sargatanas\u{A0}[Aether]</p>"#
            .replace("\\u{A0}", "\u{A0}");
        let summary = parse_character_summary(1, &html).unwrap();
        assert_eq!(summary.home_world, "Sargatanas");
    }

    #[test]
    fn missing_name_is_an_error_not_an_empty_character() {
        let html = r#"<p class="frame__chara__world">Brynhildr [Crystal]</p>"#;
        assert!(matches!(
            parse_character_summary(7, html),
            Err(ProfileError::MissingField { field: "name", .. })
        ));
    }

    #[test]
    fn missing_home_world_is_an_error() {
        let html = r#"<p class="frame__chara__name">Kalanne Ymir</p>"#;
        assert!(matches!(
            parse_character_summary(7, html),
            Err(ProfileError::MissingField {
                field: "home world",
                ..
            })
        ));
    }
}
