//! Bounded operator maintenance. See docs/recent-sales-projection.md.

use sea_orm::{ConnectOptions, Database};
use std::{collections::BTreeSet, process::ExitCode, time::Duration};
use ultros_db::{UltrosDb, recent_sales::BackfillStep};

const USAGE: &str = "recent_sales_backfill [--status | --max-boards N --statement-timeout-ms N]\nRequires DATABASE_URL and the recent-sales migration; deploy all new sale writers before backfilling.\nDefaults: 1000 board attempts, 10000 ms per board-attempt statement. See docs/recent-sales-projection.md.";

#[derive(Debug)]
struct Options {
    status: bool,
    max_boards: u32,
    timeout: Duration,
}

fn parse(args: impl Iterator<Item = String>) -> Result<Options, &'static str> {
    let mut options = Options {
        status: false,
        max_boards: 1000,
        timeout: Duration::from_secs(10),
    };
    let mut seen = BTreeSet::new();
    let mut args = args;
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option");
        }
        match arg.as_str() {
            "--status" => options.status = true,
            "--max-boards" => {
                options.max_boards = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| (1..=1_000_000).contains(n))
                    .ok_or("max-boards must be 1..=1000000")?;
            }
            "--statement-timeout-ms" => {
                let millis = args
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|n| (1..=300_000).contains(n))
                    .ok_or("statement-timeout-ms must be 1..=300000")?;
                options.timeout = Duration::from_millis(millis);
            }
            _ => return Err("unknown option; use --help"),
        }
    }
    if options.status && seen.len() > 1 {
        return Err("--status cannot be combined with backfill options");
    }
    Ok(options)
}

async fn run() -> Result<(), &'static str> {
    let options = parse(std::env::args().skip(1))?;
    // No dotenv loading, migrations, app initialization, or game-world ingest.
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let mut connection = ConnectOptions::new(url);
    connection
        .max_connections(1)
        .min_connections(0)
        .sqlx_logging(false);
    let connection = Database::connect(connection)
        .await
        .map_err(|_| "database connection failed")?;
    let db = UltrosDb::from_connection(connection);
    if !options.status {
        for _ in 0..options.max_boards {
            let step=db.backfill_recent_sales_step(options.timeout).await.map_err(|_|"backfill step failed; check PostgreSQL logs and rerun to resume committed progress")?;
            println!(
                "{}",
                serde_json::to_string(&step).map_err(|_| "cannot serialize progress")?
            );
            if step == BackfillStep::Complete {
                break;
            }
        }
    }
    let status = db
        .recent_sales_backfill_status()
        .await
        .map_err(|_| "cannot read backfill state; apply the migration first")?;
    println!(
        "{}",
        serde_json::to_string(&status).map_err(|_| "cannot serialize status")?
    );
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            println!("{}", serde_json::json!({"error":error}));
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> impl Iterator<Item = String> {
        values.iter().map(|value| (*value).to_string())
    }
    #[test]
    fn maintenance_always_has_finite_validated_bounds() {
        let defaults = parse(args(&[])).unwrap();
        assert_eq!(defaults.max_boards, 1000);
        assert_eq!(defaults.timeout, Duration::from_secs(10));
        assert!(parse(args(&["--status"])).unwrap().status);
        let explicit = parse(args(&["--max-boards", "1", "--statement-timeout-ms", "5"])).unwrap();
        assert_eq!(explicit.max_boards, 1);
        assert_eq!(explicit.timeout, Duration::from_millis(5));
        for invalid in [
            vec!["--max-boards", "0"],
            vec!["--max-boards", "1000001"],
            vec!["--max-boards", "-1"],
            vec!["--max-boards"],
            vec!["--statement-timeout-ms", "0"],
            vec!["--statement-timeout-ms", "300001"],
            vec!["--statement-timeout-ms", "nope"],
            vec!["--status", "--max-boards", "2"],
            vec!["--status", "--status"],
            vec!["--unknown"],
        ] {
            assert!(parse(args(&invalid)).is_err(), "{invalid:?}");
        }
    }
}
