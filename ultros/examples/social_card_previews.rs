//! Render catalog-backed route previews without a database or running server.
use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use ultros_app::social_card::{SocialCardHero, SocialCardKind, parse_locale, social_card_content};
use ultros_item_card::{CardContent, CardHero, CardLocale, render_card};

fn main() -> Result<()> {
    let destination = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/route-card-previews"));
    fs::create_dir_all(&destination)?;
    for code in ["en", "ja", "de", "fr", "ko", "cn", "tc"] {
        for (name, kind) in [
            ("recipe", SocialCardKind::Recipe(37872)),
            ("category", SocialCardKind::Category(1)),
            (
                "gear-detail",
                SocialCardKind::JobsetLevel("SAM".into(), 640),
            ),
        ] {
            let Some(content) = social_card_content(parse_locale(code).unwrap(), &kind, None)
            else {
                println!("SKIP {name}/{code}: unavailable in regional catalog");
                continue;
            };
            let hero = match content.hero {
                SocialCardHero::Item(id) => CardHero::Item(id),
                SocialCardHero::Job(glyph) => CardHero::Job(glyph),
                SocialCardHero::Currency => CardHero::Currency,
                SocialCardHero::Search => CardHero::Search,
                SocialCardHero::Analyzer => CardHero::Analyzer,
                SocialCardHero::Help => CardHero::Help,
            };
            let png = render_card(&CardContent {
                title: &content.title,
                subtitle: &content.subtitle,
                eyebrow: &content.eyebrow,
                footer: &content.footer,
                hero,
                locale: CardLocale::from_code(code).unwrap(),
            })
            .with_context(|| format!("rendering {name}/{code}"))?;
            let path = destination.join(format!("{name}-{code}.png"));
            fs::write(&path, png)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}
