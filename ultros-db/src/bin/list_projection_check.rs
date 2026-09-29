//! Read-only operator CLI. See docs/list-projection-checker.md.

use std::{collections::BTreeSet, process::ExitCode};

use sea_orm::{ConnectOptions, Database};
use ultros_db::list_projection_check::{Selection, check};

fn parse(args: impl Iterator<Item = String>) -> Result<Selection, &'static str> {
    let mut selection = Selection::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("every option requires a value")?;
        match arg.as_str() {
            "--ids" => selection.ids.extend(parse_ids(&value)?),
            "--ids-file" => {
                let contents = std::fs::read_to_string(value).map_err(|_| "cannot read ID file")?;
                selection.ids.extend(parse_ids(&contents)?);
            }
            "--updated-since" => {
                selection.updated_since =
                    Some(value.parse().map_err(|_| "expected RFC3339 timestamp")?);
            }
            _ => return Err("unknown option; use --ids, --ids-file, or --updated-since"),
        }
    }
    Ok(selection)
}

fn parse_ids(text: &str) -> Result<BTreeSet<i32>, &'static str> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<i32>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or("IDs must be positive i32 integers")
        })
        .collect()
}

async fn run() -> Result<ExitCode, &'static str> {
    let selection = parse(std::env::args().skip(1))?;
    // Deliberately no dotenv loading, app initialization, migration or world ingest.
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(1)
        .min_connections(0)
        .sqlx_logging(false);
    let db = Database::connect(options)
        .await
        .map_err(|_| "database connection failed")?;
    let report = check(&db, selection)
        .await
        .map_err(|_| "database snapshot read failed")?;
    let code = report.exit_code();
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|_| "report serialization failed")?
    );
    Ok(ExitCode::from(code))
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            // Never interpolate connection URLs, raw DB errors or snapshot bytes.
            println!(
                "{}",
                serde_json::json!({"format_version": 1, "error": error})
            );
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_validated_and_deduplicated() {
        assert_eq!(parse_ids("3,1\n3 2").unwrap(), BTreeSet::from([1, 2, 3]));
        for text in ["0", "-1", "2147483648", "12nope"] {
            assert!(parse_ids(text).is_err());
        }
        assert!(parse_ids("").unwrap().is_empty());
        assert!(parse(["--ids".into()].into_iter()).is_err());
        assert!(parse(["--unknown".into(), "1".into()].into_iter()).is_err());
        assert!(parse(["--updated-since".into(), "yesterday".into()].into_iter()).is_err());
    }
}
