//! Evergreen social preview content shared by page metadata and the image renderer.
//!
//! These cards describe a page, never its current market data. In particular,
//! no user preferences, live prices, rankings, or timestamps enter the content.

use crate::i18n::*;
use crate::routes::item_explorer::canonical_job_acronym;
use xiv_gen::{ClassJobCategoryId, ClassJobId, ItemId, ItemSearchCategoryId, Language, RecipeId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocialCardKind {
    Home,
    Item(i32),
    Recipe(i32),
    Category(i32),
    Jobset(String),
    JobsetLevel(String, i32),
    Currency(Option<i32>),
    Tool(String),
    Help(Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocialCardHero {
    Item(i32),
    /// A glyph in the bundled FFXIVAppIcons font, not a Unicode text symbol.
    Job(char),
    Currency,
    Search,
    Analyzer,
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocialCardContent {
    pub title: String,
    pub subtitle: String,
    pub eyebrow: String,
    pub footer: String,
    pub description: String,
    pub hero: SocialCardHero,
}

/// Only canonical locale identifiers can create cache identities.
pub fn parse_locale(value: &str) -> Option<Locale> {
    Some(match value {
        "en" => Locale::en,
        "ja" => Locale::ja,
        "de" => Locale::de,
        "fr" => Locale::fr,
        "cn" => Locale::cn,
        "ko" => Locale::ko,
        "tc" => Locale::tc,
        _ => return None,
    })
}

pub fn game_language(locale: Locale) -> Language {
    match locale {
        Locale::en => Language::En,
        Locale::ja => Language::Ja,
        Locale::de => Language::De,
        Locale::fr => Language::Fr,
        Locale::cn => Language::Cn,
        Locale::ko => Language::Ko,
        Locale::tc => Language::Tc,
    }
}

pub fn og_locale(locale: Locale) -> &'static str {
    match locale {
        Locale::en => "en_US",
        Locale::ja => "ja_JP",
        Locale::de => "de_DE",
        Locale::fr => "fr_FR",
        Locale::cn => "zh_CN",
        Locale::ko => "ko_KR",
        Locale::tc => "zh_TW",
    }
}

const TOOLS: &[&str] = &[
    "flip-finder",
    "vendor-resale",
    "recipe-analyzer",
    "fc-crafting-analyzer",
    "leve-analyzer",
    "scrip-sources",
    "venture-analyzer",
    "trends",
    "items",
    "about",
    "bot",
    "changelog",
    "privacy",
    "cookie-policy",
];

const HELP_TOPICS: &[&str] = &[
    "getting-started",
    "flip-finder",
    "vendor-resale",
    "recipe-analyzer",
    "leve-analyzer",
    "fc-crafting",
    "scrip-sources",
    "venture-analyzer",
    "market-trends",
    "lists-alerts-retainers",
];

fn positive_id(value: &str) -> Option<i32> {
    value.parse::<i32>().ok().filter(|id| *id > 0)
}

fn canonical_job(value: &str) -> Option<&'static str> {
    (1..=43)
        .filter_map(|id| canonical_job_acronym(ClassJobId(id)))
        .find(|acronym| acronym.eq_ignore_ascii_case(value))
}

fn sentence_case(value: String) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => value,
    }
}

impl SocialCardKind {
    /// Parse only public page identities. Private and unknown paths use the
    /// public homepage card, without reflecting any path or query text.
    pub fn from_route(path: &str) -> Self {
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let segments: Vec<_> = path.trim_matches('/').split('/').collect();
        let kind = match segments.as_slice() {
            [""] => Some(Self::Home),
            ["item", id] | ["item", _, id] => positive_id(id).map(Self::Item),
            ["recipe", id] => positive_id(id).map(Self::Recipe),
            ["items", "jobset", job, "set", level] => canonical_job(job).and_then(|job| {
                positive_id(level).map(|level| Self::JobsetLevel(job.to_string(), level))
            }),
            ["items", "jobset", job] => canonical_job(job).map(|job| Self::Jobset(job.to_string())),
            ["items", "category", id] => Some(
                positive_id(id)
                    .map(Self::Category)
                    .unwrap_or_else(|| Self::Tool("items".to_string())),
            ),
            ["currency-exchange"] => Some(Self::Currency(None)),
            ["currency-exchange", id] => positive_id(id).map(|id| Self::Currency(Some(id))),
            ["help"] => Some(Self::Help(None)),
            ["help", topic] if HELP_TOPICS.contains(topic) => {
                Some(Self::Help(Some((*topic).to_string())))
            }
            ["analyzer"] | ["analyzer", _] => Some(Self::Tool("flip-finder".to_string())),
            [tool] if TOOLS.contains(tool) => Some(Self::Tool((*tool).to_string())),
            [
                tool @ ("flip-finder" | "vendor-resale" | "fc-crafting-analyzer" | "trends"),
                _,
            ] => Some(Self::Tool((*tool).to_string())),
            _ => None,
        };
        kind.unwrap_or(Self::Home)
    }

    /// Parse a versioned image URL. Unknown keys are rejected instead of
    /// minting unlimited cache entries for generic fallback images.
    pub fn from_parts(kind: &str, key: &str) -> Option<Self> {
        match kind {
            "home" if key == "default" => Some(Self::Home),
            "item" => positive_id(key).map(Self::Item),
            "recipe" => positive_id(key).map(Self::Recipe),
            "category" => positive_id(key).map(Self::Category),
            "jobset-level" => {
                let (job, level) = key.split_once('-')?;
                Some(Self::JobsetLevel(
                    canonical_job(job)?.to_string(),
                    positive_id(level)?,
                ))
            }
            "jobset" => canonical_job(key).map(|job| Self::Jobset(job.to_string())),
            "currency" if key == "default" => Some(Self::Currency(None)),
            "currency" => positive_id(key).map(|id| Self::Currency(Some(id))),
            "tool" if TOOLS.contains(&key) => Some(Self::Tool(key.to_string())),
            "help" if key == "default" => Some(Self::Help(None)),
            "help" if HELP_TOPICS.contains(&key) => Some(Self::Help(Some(key.to_string()))),
            _ => None,
        }
    }

    pub fn parts(&self) -> (&'static str, String) {
        match self {
            Self::Home => ("home", "default".to_string()),
            Self::Item(id) => ("item", id.to_string()),
            Self::Recipe(id) => ("recipe", id.to_string()),
            Self::Category(id) => ("category", id.to_string()),
            Self::JobsetLevel(job, level) => ("jobset-level", format!("{job}-{level}")),
            Self::Jobset(job) => ("jobset", job.to_string()),
            Self::Currency(id) => (
                "currency",
                id.map_or_else(|| "default".to_string(), |id| id.to_string()),
            ),
            Self::Tool(tool) => ("tool", tool.to_string()),
            Self::Help(topic) => (
                "help",
                topic.clone().unwrap_or_else(|| "default".to_string()),
            ),
        }
    }
}

fn tool_copy(locale: Locale, slug: &str) -> Option<(String, String)> {
    macro_rules! copy {
        ($title:ident, $subtitle:ident) => {
            (
                td_string!(locale, $title).to_string(),
                td_string!(locale, $subtitle).to_string(),
            )
        };
    }
    Some(match slug {
        "flip-finder" => copy!(flip_finder, flip_finder_desc),
        "vendor-resale" => copy!(vendor_resale, vendor_resale_desc),
        "recipe-analyzer" => copy!(recipe_analyzer, recipe_analyzer_desc),
        "fc-crafting-analyzer" => copy!(fc_crafting_analyzer_title, fc_crafting_desc),
        "leve-analyzer" => copy!(leve_analyzer, leve_analyzer_desc),
        "scrip-sources" => copy!(scrip_sources, scrip_sources_desc),
        "venture-analyzer" => copy!(venture_analyzer, venture_analyzer_desc),
        "trends" => copy!(market_trends, market_trends_desc),
        "items" => copy!(item_explorer, item_explorer_desc),
        "about" => copy!(about, social_card_about_subtitle),
        "bot" => copy!(discord_bot, discord_bot_desc),
        "changelog" => copy!(changelog_page_heading, social_card_changelog_subtitle),
        "privacy" => copy!(privacy_policy_title, social_card_privacy_subtitle),
        "cookie-policy" => copy!(cookie_policy_title, social_card_cookie_subtitle),
        _ => return None,
    })
}

/// Build localized, deterministic content. The caller must validate `world`
/// against world data; a missing world always uses neutral, public context.
pub fn social_card_content(
    locale: Locale,
    kind: &SocialCardKind,
    world: Option<&str>,
) -> Option<SocialCardContent> {
    #[cfg(feature = "ssr")]
    let data = || xiv_gen_db::data_for(game_language(locale));
    #[cfg(not(feature = "ssr"))]
    let data = crate::global_state::xiv_data::tracked_data;

    // A regional pack can lag the global release. Use only that pack on both
    // the server and browser; page metadata chooses a localized home preview
    // when the requested entity is unavailable instead of changing on hydrate.
    let lookup_item = |id: i32| {
        if id <= 0 {
            return None;
        }
        data()
            .items
            .get(&ItemId(id))
            .filter(|item| !item.name.trim().is_empty())
    };

    let mut content = SocialCardContent {
        title: td_string!(locale, social_card_home_title).to_string(),
        subtitle: td_string!(locale, social_card_home_subtitle).to_string(),
        eyebrow: td_string!(locale, social_card_eyebrow).to_string(),
        footer: td_string!(locale, social_card_footer).to_string(),
        description: td_string!(locale, social_card_home_description).to_string(),
        hero: SocialCardHero::Search,
    };

    match kind {
        SocialCardKind::Home => return Some(content),
        SocialCardKind::Item(id) => {
            let item = lookup_item(*id)?;
            content.title = item.name.clone();
            content.subtitle = td_string!(locale, social_card_item_subtitle).to_string();
            content.description =
                td_string!(locale, social_card_item_description, item = &item.name).to_string();
            content.hero = SocialCardHero::Item(*id);
        }
        SocialCardKind::Recipe(id) => {
            let recipe = data().recipes.get(&RecipeId(*id))?;
            let item = lookup_item(recipe.item_result)?;
            content.title = item.name.clone();
            content.subtitle = td_string!(locale, social_card_recipe_subtitle).to_string();
            content.eyebrow = td_string!(locale, social_card_recipe_eyebrow).to_string();
            content.description =
                td_string!(locale, social_card_recipe_description, item = &item.name).to_string();
            content.hero = SocialCardHero::Item(item.key_id.0);
        }
        SocialCardKind::Category(id) => {
            let category = data()
                .item_search_categorys
                .get(&ItemSearchCategoryId(*id))
                .filter(|category| !category.name.trim().is_empty())?;
            content.title = category.name.clone();
            content.subtitle = td_string!(locale, social_card_item_subtitle).to_string();
            content.description =
                td_string!(locale, category_list_desc).replace("%category%", &category.name);
            content.hero = SocialCardHero::Search;
        }
        SocialCardKind::Jobset(acronym) | SocialCardKind::JobsetLevel(acronym, _) => {
            let id = (1..=43).map(ClassJobId).find(|id| {
                canonical_job_acronym(*id)
                    .is_some_and(|canonical| canonical.eq_ignore_ascii_case(acronym))
            })?;
            let job = data()
                .class_jobs
                .get(&id)
                .filter(|job| !job.name.trim().is_empty());
            let job = job?;
            content.title = sentence_case(
                td_string!(locale, social_card_jobset_title, job = &job.name).to_string(),
            );
            content.subtitle = td_string!(locale, social_card_jobset_subtitle).to_string();
            content.eyebrow = td_string!(locale, social_card_jobset_eyebrow).to_string();
            content.description =
                td_string!(locale, social_card_jobset_description, job = &job.name).to_string();
            content.footer = sentence_case(job.name.clone());
            if let SocialCardKind::JobsetLevel(_, level) = kind {
                let acronym = canonical_job(acronym)?;
                let catalog = data();
                // Validate the gear itself, not a shared name prefix: localized
                // item names may put the set name at different positions.
                let pieces = catalog
                    .items
                    .values()
                    .filter(|item| {
                        item.level_item == *level
                            && item.item_search_category > 0
                            && catalog
                                .class_job_categorys
                                .get(&ClassJobCategoryId(item.class_job_category))
                                .is_some_and(|category| {
                                    crate::routes::item_explorer::job_category_lookup(
                                        category, acronym,
                                    )
                                })
                    })
                    .take(2)
                    .count();
                if pieces < 2 {
                    return None;
                }
                let level = format!("{} {level}", td_string!(locale, item_explorer_ilvl_prefix));
                content.subtitle = format!("{level} · {}", content.subtitle);
                content.description = format!("{level} · {}", content.description);
            }
            // xivicon.css maps decimal job 34 to U+F034 (not U+F022).
            // The shipped font currently contains jobs 1..=42 only.
            content.hero = if (1..=42).contains(&id.0) {
                let codepoint = 0xf000 + (id.0 / 10 * 16 + id.0 % 10) as u32;
                SocialCardHero::Job(char::from_u32(codepoint)?)
            } else {
                SocialCardHero::Analyzer
            };
        }
        SocialCardKind::Currency(id) => {
            content.title = td_string!(locale, currency_exchange).to_string();
            content.subtitle = td_string!(locale, social_card_currency_subtitle).to_string();
            content.eyebrow = td_string!(locale, social_card_currency_eyebrow).to_string();
            content.description = td_string!(locale, social_card_currency_description).to_string();
            content.hero = SocialCardHero::Currency;
            if let Some(id) = id {
                let item = lookup_item(*id)?;
                content.title = item.name.clone();
                content.description = td_string!(
                    locale,
                    social_card_exchange_item_description,
                    item = &item.name
                )
                .to_string();
                content.hero = SocialCardHero::Item(*id);
            }
        }
        SocialCardKind::Tool(slug) => {
            (content.title, content.subtitle) = tool_copy(locale, slug)?;
            content.eyebrow = td_string!(locale, social_card_tool_eyebrow).to_string();
            content.description = format!("{} · Ultros — {}", content.title, content.subtitle);
            content.hero = if matches!(slug.as_str(), "items" | "about") {
                SocialCardHero::Search
            } else {
                SocialCardHero::Analyzer
            };
        }
        SocialCardKind::Help(topic) => {
            content.title = td_string!(locale, social_card_help_title).to_string();
            content.subtitle = td_string!(locale, social_card_help_subtitle).to_string();
            content.description = td_string!(locale, social_card_help_description).to_string();
            content.eyebrow = td_string!(locale, social_card_help_eyebrow).to_string();
            content.hero = SocialCardHero::Help;
            if let Some(topic) = topic {
                let (title, subtitle) = match topic.as_str() {
                    "getting-started" => (
                        td_string!(locale, social_card_getting_started).to_string(),
                        td_string!(locale, social_card_getting_started_subtitle).to_string(),
                    ),
                    "lists-alerts-retainers" => (
                        td_string!(locale, social_card_saved_tools).to_string(),
                        td_string!(locale, social_card_saved_tools_subtitle).to_string(),
                    ),
                    "fc-crafting" => tool_copy(locale, "fc-crafting-analyzer")?,
                    "market-trends" => tool_copy(locale, "trends")?,
                    slug if HELP_TOPICS.contains(&slug) => tool_copy(locale, slug)?,
                    _ => return None,
                };
                content.title = title;
                content.subtitle = subtitle;
                content.description = format!(
                    "{} · {} — {}",
                    content.title,
                    td_string!(locale, help_meta_title),
                    content.subtitle
                );
            }
            return Some(content);
        }
    }
    if let Some(world) = world.filter(|world| !world.is_empty()) {
        content.footer = world.to_string();
    }
    Some(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_route_and_image_identities_round_trip() {
        for route in [
            "/",
            "/item/Gilgamesh/5333",
            "/recipe/37872",
            "/items/jobset/SAM/set/720",
            "/items/category/1",
            "/currency-exchange",
            "/currency-exchange/28",
            "/help/getting-started",
            "/help",
        ] {
            let kind = SocialCardKind::from_route(route);
            let (name, key) = kind.parts();
            assert_eq!(SocialCardKind::from_parts(name, &key), Some(kind));
        }
        for tool in TOOLS {
            let kind = SocialCardKind::from_route(&format!("/{tool}"));
            assert_eq!(kind, SocialCardKind::Tool(tool.to_string()));
        }
    }

    #[test]
    fn private_and_invalid_paths_never_reflect_personal_content() {
        for path in [
            "/list/secret-list-id",
            "/group/invite/secret-token",
            "/retainers/listings/123",
            "/profile?username=someone",
            "/item/0",
            "/recipe/0",
            "/recipe/not-a-recipe",
            "/items/jobset/SAM/set/0",
            "/items/jobset/SAM/set/invalid",
            "/item/not-an-item",
            "/items/jobset/NOT_A_JOB",
            "/help/unpublished",
            "/unknown-page",
        ] {
            assert_eq!(SocialCardKind::from_route(path), SocialCardKind::Home);
        }
        assert_eq!(SocialCardKind::from_parts("recipe", "-1"), None);
        assert_eq!(SocialCardKind::from_parts("category", "0"), None);
        assert_eq!(SocialCardKind::from_parts("jobset-level", "SAM-1-2"), None);
        assert_eq!(
            SocialCardKind::from_parts("jobset-level", "UNKNOWN-720"),
            None
        );
        assert_eq!(SocialCardKind::from_parts("tool", "unknown"), None);
        assert_eq!(SocialCardKind::from_parts("home", "arbitrary"), None);
        assert_eq!(parse_locale("zh-CN"), None);
        assert_eq!(parse_locale("unknown"), None);
    }

    #[test]
    fn new_routes_use_specific_card_identities() {
        assert_eq!(
            SocialCardKind::from_route(
                "/recipe/37872?world=Gilgamesh&craft=44033%3A5652&quantity=1"
            ),
            SocialCardKind::Recipe(37872)
        );
        assert_eq!(
            SocialCardKind::from_route("/items/category/1"),
            SocialCardKind::Category(1)
        );
        assert_eq!(
            SocialCardKind::from_route("/items/jobset/sam/set/720"),
            SocialCardKind::JobsetLevel("SAM".into(), 720)
        );
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn recipe_category_and_gear_detail_cards_use_localized_catalog_entities() {
        for code in ["en", "ja", "de", "fr", "cn", "ko", "tc"] {
            let locale = parse_locale(code).unwrap();
            let data = xiv_gen_db::data_for(game_language(locale));
            // ARR fixtures also used by the crawler matrix, present in every pack.
            let recipe = &data.recipes[&RecipeId(1)];
            let item = &data.items[&ItemId(recipe.item_result)];
            let card = social_card_content(locale, &SocialCardKind::Recipe(recipe.key_id.0), None)
                .unwrap();
            assert_eq!(card.title, item.name);
            assert_eq!(card.hero, SocialCardHero::Item(item.key_id.0));
            assert!(card.description.contains(&item.name));
            assert_ne!(
                card.subtitle,
                social_card_content(locale, &SocialCardKind::Item(item.key_id.0), None)
                    .unwrap()
                    .subtitle
            );
            let category = &data.item_search_categorys[&ItemSearchCategoryId(1)];
            let card =
                social_card_content(locale, &SocialCardKind::Category(category.key_id.0), None)
                    .unwrap();
            assert_eq!(card.title, category.name);
            assert!(card.description.contains(&category.name));
            let card = social_card_content(
                locale,
                &SocialCardKind::JobsetLevel("SAM".into(), 640),
                None,
            )
            .unwrap();
            assert!(card.subtitle.contains("640"));
            assert!(card.description.contains("640"));
            assert_eq!(card.hero, SocialCardHero::Job('\u{f034}'));
            for kind in [
                SocialCardKind::Recipe(i32::MAX),
                SocialCardKind::Category(i32::MAX),
                SocialCardKind::JobsetLevel("SAM".into(), i32::MAX),
            ] {
                assert!(social_card_content(locale, &kind, None).is_none());
            }
        }
        let data = xiv_gen_db::data_for(Language::En);
        let recipe = &data.recipes[&RecipeId(37872)];
        let card = social_card_content(Locale::en, &SocialCardKind::Recipe(37872), None).unwrap();
        assert_eq!(card.title, data.items[&ItemId(recipe.item_result)].name);
        assert_eq!(card.hero, SocialCardHero::Item(recipe.item_result));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn every_locale_uses_its_game_data_and_translated_content() {
        for code in ["en", "ja", "de", "fr", "cn", "ko", "tc"] {
            let locale = parse_locale(code).unwrap();
            let data = xiv_gen_db::data_for(game_language(locale));
            let item = data.items.get(&ItemId(5333)).unwrap();
            let card = social_card_content(locale, &SocialCardKind::Item(5333), None).unwrap();
            assert_eq!(card.title, item.name);
            assert!(card.description.contains(&item.name));
            assert_eq!(card.footer, td_string!(locale, social_card_footer));
            let job = data.class_jobs.get(&ClassJobId(34)).unwrap();
            let card =
                social_card_content(locale, &SocialCardKind::Jobset("SAM".into()), None).unwrap();
            assert!(card.title.to_lowercase().contains(&job.name.to_lowercase()));
            assert_eq!(card.hero, SocialCardHero::Job('\u{f034}'));
            for kind in [
                SocialCardKind::Home,
                SocialCardKind::Currency(None),
                SocialCardKind::Currency(Some(28)),
                SocialCardKind::Help(None),
            ] {
                let card = social_card_content(locale, &kind, None).unwrap();
                assert!(!card.title.is_empty());
                assert!(!card.subtitle.is_empty());
                assert!(!card.description.contains("{{"));
            }
            for tool in TOOLS {
                assert!(
                    social_card_content(locale, &SocialCardKind::Tool((*tool).into()), None)
                        .is_some()
                );
            }
            for topic in HELP_TOPICS {
                assert!(
                    social_card_content(locale, &SocialCardKind::Help(Some((*topic).into())), None)
                        .is_some()
                );
            }
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn entities_missing_from_regional_packs_do_not_substitute_english_names() {
        let english = xiv_gen_db::data_for(Language::En);
        for locale in [Locale::cn, Locale::ko, Locale::tc] {
            let regional = xiv_gen_db::data_for(game_language(locale));
            // Some releases have identical packs. Exercise the policy whenever
            // this release actually has a named item missing in a region.
            let missing = english.items.iter().find(|(id, item)| {
                id.0 > 0
                    && !item.name.trim().is_empty()
                    && regional
                        .items
                        .get(*id)
                        .is_none_or(|item| item.name.trim().is_empty())
            });
            if let Some((id, _)) = missing {
                for kind in [
                    SocialCardKind::Item(id.0),
                    SocialCardKind::Currency(Some(id.0)),
                ] {
                    assert!(social_card_content(locale, &kind, None).is_none());
                }
            }
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn missing_items_are_rejected_and_world_context_is_explicit() {
        for id in [0, -1, i32::MAX] {
            assert!(social_card_content(Locale::en, &SocialCardKind::Item(id), None).is_none());
            assert!(
                social_card_content(Locale::en, &SocialCardKind::Currency(Some(id)), None)
                    .is_none()
            );
        }
        let card = social_card_content(Locale::en, &SocialCardKind::Item(5333), Some("Gilgamesh"))
            .unwrap();
        assert_eq!(card.footer, "Gilgamesh");
    }
}
