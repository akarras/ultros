//! Reproducible full-size and chat-size visual QA, without a running server.
use std::{fs, path::PathBuf};

use anyhow::Result;
use ultros_item_card::{CardContent, CardHero, CardLocale, render_card};

fn main() -> Result<()> {
    let destination = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/social-card-previews"));
    fs::create_dir_all(&destination)?;
    let cards = [
        (
            "item-en",
            CardLocale::En,
            "Courtly Lover’s Wristlet of Aiming",
            "Compare listings across worlds",
            "FFXIV MARKET BOARD",
            "North America",
            CardHero::Item(49318),
        ),
        (
            "item-ja",
            CardLocale::Ja,
            "コートリーラヴァー・レンジャーブレスレット",
            "ワールド間の出品を比較",
            "FFXIV マーケットボード",
            "日本",
            CardHero::Item(49318),
        ),
        (
            "item-de",
            CardLocale::De,
            "Verbessertes Zeremonielles Handgelenkband des Waldläufers",
            "Angebote auf verschiedenen Welten vergleichen",
            "FFXIV MARKTBRETT",
            "Europa",
            CardHero::Item(49318),
        ),
        (
            "item-fr",
            CardLocale::Fr,
            "Bracelet de pisteur des amoureux de la cour",
            "Comparez les offres entre les Mondes",
            "TABLEAU DES VENTES FFXIV",
            "Europe",
            CardHero::Item(49318),
        ),
        (
            "item-ko",
            CardLocale::Ko,
            "궁정 연인의 유격대 팔찌",
            "서버 간 판매 목록 비교",
            "FFXIV 장터 게시판",
            "한국",
            CardHero::Item(49318),
        ),
        (
            "item-cn",
            CardLocale::Cn,
            "宫廷恋人精准手镯",
            "比较不同服务器的在售物品",
            "FFXIV 市场布告板",
            "中国",
            CardHero::Item(49318),
        ),
        (
            "item-tc",
            CardLocale::Tc,
            "宮廷戀人精準手鐲",
            "比較不同伺服器的在售物品",
            "FFXIV 市場佈告欄",
            "繁體中文",
            CardHero::Item(49318),
        ),
        (
            "samurai",
            CardLocale::En,
            "Samurai gear sets",
            "Compare gear across worlds",
            "FFXIV GEAR SETS",
            "Samurai",
            CardHero::Job('\u{f034}'),
        ),
        (
            "currency",
            CardLocale::En,
            "Currency Exchange",
            "Find what your currency can buy",
            "FFXIV CURRENCY TOOLS",
            "Currency tools",
            CardHero::Currency,
        ),
        (
            "home",
            CardLocale::En,
            "Your next market board advantage.",
            "Compare prices. Plan your purchases.",
            "FINAL FANTASY XIV",
            "Final Fantasy XIV",
            CardHero::Search,
        ),
        (
            "analyzer",
            CardLocale::En,
            "Find your next market opportunity",
            "Explore items across worlds",
            "FFXIV MARKET TOOLS",
            "Market Analyzer",
            CardHero::Analyzer,
        ),
        (
            "help",
            CardLocale::En,
            "Make the most of Ultros",
            "Guides for your market board journey",
            "FFXIV GUIDES",
            "Help & guides",
            CardHero::Help,
        ),
    ];
    for (name, locale, title, subtitle, eyebrow, footer, hero) in cards {
        let png = render_card(&CardContent {
            title,
            subtitle,
            eyebrow,
            footer,
            hero,
            locale,
        })?;
        fs::write(destination.join(format!("{name}.png")), &png)?;
        image::load_from_memory(&png)?
            .resize_exact(400, 210, image::imageops::FilterType::Lanczos3)
            .save(destination.join(format!("{name}-400.png")))?;
        println!("{}", destination.join(format!("{name}.png")).display());
    }
    Ok(())
}
