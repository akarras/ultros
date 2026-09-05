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

fn compile(sources: &[(String, String)]) -> Result<String, String> {
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
    let mut output = String::from("pub static CHANGELOG: &[ChangelogEntry] = &[\n");
    for (
        _,
        Parsed {
            date,
            entry,
            category,
            importance,
            ..
        },
    ) in entries
    {
        // Debug escaping emits Rust string literals, including quotes and Unicode.
        writeln!(output,
            "ChangelogEntry {{ date: {date:?}, category: ChangelogCategory::{category}, importance: ChangelogImportance::{importance}, title: {:?}, blurb: {:?}, link: {:?} }},",
            entry.title, entry.blurb, entry.link
        ).unwrap();
    }
    output.push_str("];\n");
    Ok(output)
}

fn generate(directory: &Path) -> Result<String, Box<dyn std::error::Error>> {
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
    Ok(compile(&sources)?)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("changes");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("changelog.rs");
    fs::write(output, generate(&directory)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, importance: &str) -> String {
        serde_json::json!({
            "category": "features", "importance": importance,
            "title": title, "blurb": "A player-facing change."
        })
        .to_string()
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
            compile(&sources.into_iter().rev().collect::<Vec<_>>()).unwrap()
        );
        let positions = ["High first", "High second", "Medium", "Low", "Old high"]
            .map(|title| output.find(&format!("title: {title:?}")).unwrap());
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
        assert!(output.contains(r#"title: "Quotes \" \\ 日本語\n<script>""#));
        assert!(output.contains(r#"Some("/items?search=test#results")"#));
    }

    #[test]
    fn compiles_checked_in_changes_and_handles_an_empty_list() {
        assert!(
            generate(&Path::new(env!("CARGO_MANIFEST_DIR")).join("changes"))
                .unwrap()
                .contains("ChangelogEntry {")
        );
        assert!(!compile(&[]).unwrap().contains("ChangelogEntry {"));
    }
}
