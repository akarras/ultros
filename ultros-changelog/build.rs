use serde::Deserialize;
use std::{fmt::Write, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    category: String,
    importance: String,
    title: String,
    blurb: String,
    link: Option<String>,
    /// Lives behind a Labs toggle. Shown with a badge and never counted as
    /// "what's new", since most players cannot see the change yet.
    #[serde(default)]
    labs: bool,
}

struct Parsed<'a> {
    date: &'a str,
    entry: Entry,
    category: &'static str,
    importance: &'static str,
    rank: u8,
}

fn parse<'a>(name: &'a str, source: &str) -> Result<Parsed<'a>, String> {
    let stem = name.strip_suffix(".json").ok_or("expected a .json file")?;
    let date = stem
        .get(..10)
        .ok_or("expected YYYY-MM-DD-description.json")?;
    let slug = stem
        .get(11..)
        .ok_or("expected YYYY-MM-DD-description.json")?;
    if stem.as_bytes()[10] != b'-'
        || slug.is_empty()
        || !slug
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || slug.starts_with('-')
        || slug.ends_with('-')
    {
        return Err("expected YYYY-MM-DD-description.json with a lowercase description".into());
    }
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| "filename must start with a valid YYYY-MM-DD date")?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        return Err("filename date must be zero-padded YYYY-MM-DD".into());
    }
    let entry: Entry = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let category = match entry.category.as_str() {
        "features" => "Features",
        "improvements" => "Improvements",
        "bug_fixes" => "BugFixes",
        _ => return Err("category must be features, improvements, or bug_fixes".into()),
    };
    let (rank, importance) = match entry.importance.as_str() {
        "high" => (0, "High"),
        "medium" => (1, "Medium"),
        "low" => (2, "Low"),
        _ => return Err("importance must be high, medium, or low".into()),
    };
    if entry.title.trim().is_empty() || entry.blurb.trim().is_empty() {
        return Err("title and blurb must not be empty".into());
    }
    if let Some(link) = &entry.link
        && (!link.starts_with('/')
            || link.starts_with("//")
            || link.contains('\\')
            || link.chars().any(|c| c.is_whitespace() || c.is_control()))
    {
        return Err("link must be an internal app route beginning with a single /".into());
    }
    Ok(Parsed {
        date,
        entry,
        category,
        importance,
        rank,
    })
}

/// Every change, newest day first, then high/medium/low, then filename.
fn sorted<'a>(sources: &'a [(String, String)]) -> Result<Vec<Parsed<'a>>, String> {
    let mut entries = sources
        .iter()
        .map(|(name, source)| {
            parse(name, source)
                .map(|parsed| (name, parsed))
                .map_err(|error| format!("{name}: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|(a_name, a), (b_name, b)| {
        b.date
            .cmp(a.date)
            .then(a.rank.cmp(&b.rank))
            .then(a_name.cmp(b_name))
    });
    Ok(entries.into_iter().map(|(_, parsed)| parsed).collect())
}

/// The server-only `CHANGELOG` table, behind the crate's `history` feature.
fn entries_source(entries: &[Parsed<'_>]) -> String {
    let mut output = String::from("pub static CHANGELOG: &[ChangelogEntry] = &[\n");
    for Parsed {
        date,
        entry,
        category,
        importance,
        ..
    } in entries
    {
        // Debug escaping emits Rust string literals, including quotes and Unicode.
        writeln!(output,
            "ChangelogEntry {{ date: Cow::Borrowed({date:?}), category: ChangelogCategory::{category}, importance: ChangelogImportance::{importance}, title: Cow::Borrowed({:?}), blurb: Cow::Borrowed({:?}), link: {}, labs: {} }},",
            entry.title,
            entry.blurb,
            match &entry.link {
                Some(link) => format!("Some(Cow::Borrowed({link:?}))"),
                None => "None".to_string(),
            },
            entry.labs
        ).unwrap();
    }
    output.push_str("];\n");
    output
}

/// The two dates the client needs. These are all the wasm bundle gets: a
/// `&str` each, rather than the whole table the server serves.
fn dates_source(entries: &[Parsed<'_>]) -> String {
    let latest = entries.first().map(|parsed| parsed.date).unwrap_or("");
    // Skip Labs-only days so a Labs release does not light up the
    // what's-new dot for players who have not opted in.
    let announced = entries
        .iter()
        .find(|parsed| !parsed.entry.labs)
        .map(|parsed| parsed.date)
        .unwrap_or("");
    format!(
        "pub const LATEST: &str = {latest:?};\npub const LATEST_ANNOUNCED: &str = {announced:?};\n"
    )
}

fn generate(directory: &Path) -> Result<(String, String), Box<dyn std::error::Error>> {
    // Watching the directory catches additions and removals as well as edits.
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut sources = Vec::new();
    for file in fs::read_dir(directory)? {
        let path = file?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            sources.push((
                path.file_name()
                    .unwrap()
                    .to_str()
                    .ok_or("non-UTF-8 filename")?
                    .to_owned(),
                fs::read_to_string(&path)?,
            ));
        }
    }
    let entries = sorted(&sources)?;
    Ok((entries_source(&entries), dates_source(&entries)))
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("changes");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let (entries, dates) = generate(&directory)?;
    fs::write(out.join("changelog.rs"), entries)?;
    fs::write(out.join("dates.rs"), dates)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(sources: &[(String, String)]) -> Result<String, String> {
        sorted(sources).map(|entries| entries_source(&entries))
    }

    fn dates(sources: &[(String, String)]) -> String {
        dates_source(&sorted(sources).unwrap())
    }

    fn entry(title: &str, importance: &str) -> String {
        serde_json::json!({
            "category": "features", "importance": importance,
            "title": title, "blurb": "A player-facing change."
        })
        .to_string()
    }

    fn labs_entry(title: &str) -> String {
        let mut source: serde_json::Value = serde_json::from_str(&entry(title, "high")).unwrap();
        source["labs"] = true.into();
        source.to_string()
    }

    #[test]
    fn orders_by_date_importance_then_filename() {
        let sources = vec![
            ("2026-09-03-old.json".into(), entry("Old high", "high")),
            ("2026-09-04-a-low.json".into(), entry("Low", "low")),
            (
                "2026-09-04-z-high.json".into(),
                entry("High second", "high"),
            ),
            ("2026-09-04-b-high.json".into(), entry("High first", "high")),
            ("2026-09-04-c-medium.json".into(), entry("Medium", "medium")),
        ];
        let output = compile(&sources).unwrap();
        assert_eq!(
            output,
            compile(&sources.iter().cloned().rev().collect::<Vec<_>>()).unwrap()
        );
        let positions = ["High first", "High second", "Medium", "Low", "Old high"].map(|title| {
            output
                .find(&format!("title: Cow::Borrowed({title:?})"))
                .unwrap()
        });
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(output.matches("ChangelogEntry {").count(), 5);
    }

    #[test]
    fn validates_filenames_and_content_with_source_names() {
        let valid = entry("Hello", "medium");
        for name in [
            "pr-123.json",
            "2026-02-30-fix.json",
            "2026-9-04-fix.json",
            "2026-09-04-.json",
        ] {
            assert!(
                compile(&[(name.into(), valid.clone())])
                    .unwrap_err()
                    .starts_with(name)
            );
        }
        for source in [
            valid.replace("features", "feature"),
            valid.replace("medium", "urgent"),
            valid.replace("importance", "priority"),
            valid.replace("Hello", " "),
            valid.replace("blurb", "unknown"),
        ] {
            assert!(parse("2026-09-04-change.json", &source).is_err());
        }
        assert!(parse("2024-02-29-change.json", &valid).is_ok());
    }

    #[test]
    fn escapes_text_and_checks_links() {
        let mut source: serde_json::Value =
            serde_json::from_str(&entry("Quotes \" \\ 日本語\n<script>", "low")).unwrap();
        for link in [
            "https://example.com",
            "//example.com",
            "/\\example.com",
            "/bad\nlink",
        ] {
            source["link"] = link.into();
            assert!(parse("2026-09-04-change.json", &source.to_string()).is_err());
        }
        source["link"] = "/items?search=test#results".into();
        let output = compile(&[("2026-09-04-change.json".into(), source.to_string())]).unwrap();
        assert!(output.contains(r#"title: Cow::Borrowed("Quotes \" \\ 日本語\n<script>")"#));
        assert!(output.contains(r#"Some(Cow::Borrowed("/items?search=test#results"))"#));
    }

    #[test]
    fn labs_flag_defaults_off_and_must_be_a_bool() {
        let plain = entry("Plain", "medium");
        assert!(
            compile(&[("2026-09-04-change.json".into(), plain.clone())])
                .unwrap()
                .contains("labs: false")
        );
        let mut source: serde_json::Value = serde_json::from_str(&plain).unwrap();
        source["labs"] = true.into();
        assert!(
            compile(&[("2026-09-04-change.json".into(), source.to_string())])
                .unwrap()
                .contains("labs: true")
        );
        source["labs"] = "yes".into();
        assert!(parse("2026-09-04-change.json", &source.to_string()).is_err());
    }

    /// The client-side dates are the newest day overall and the newest day
    /// carrying a change everyone can see. A Labs-only day moves the first
    /// and not the second.
    #[test]
    fn dates_report_the_newest_day_and_the_newest_announced_day() {
        let sources = vec![
            ("2026-09-03-shipped.json".into(), entry("Shipped", "high")),
            ("2026-09-05-secret.json".into(), labs_entry("Behind a flag")),
        ];
        assert_eq!(
            dates(&sources),
            "pub const LATEST: &str = \"2026-09-05\";\n\
             pub const LATEST_ANNOUNCED: &str = \"2026-09-03\";\n"
        );
    }

    /// With nothing to announce both dates are empty strings, which sort
    /// before every real date, so the what's-new dot stays off.
    #[test]
    fn dates_are_empty_without_entries_or_without_an_announced_one() {
        assert_eq!(
            dates(&[]),
            "pub const LATEST: &str = \"\";\npub const LATEST_ANNOUNCED: &str = \"\";\n"
        );
        let labs_only = vec![("2026-09-05-secret.json".into(), labs_entry("Only Labs"))];
        assert_eq!(
            dates(&labs_only),
            "pub const LATEST: &str = \"2026-09-05\";\n\
             pub const LATEST_ANNOUNCED: &str = \"\";\n"
        );
    }

    #[test]
    fn compiles_checked_in_changes_and_handles_an_empty_list() {
        let (entries, dates) =
            generate(&Path::new(env!("CARGO_MANIFEST_DIR")).join("changes")).unwrap();
        assert!(entries.contains("ChangelogEntry {"));
        assert!(dates.contains("pub const LATEST: &str = \"20"));
        assert!(!compile(&[]).unwrap().contains("ChangelogEntry {"));
    }
}
