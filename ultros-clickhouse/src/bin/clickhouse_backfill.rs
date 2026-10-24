//! Resume a historical backfill or replay a bounded range to repair gaps.
//! See docs/ingest-observability.md for the recovery procedure.

use ultros_clickhouse::{
    ClickHouseClient,
    backfill::{BackfillOptions, backfill_sales_with_options},
};
use ultros_db::UltrosDb;

const USAGE: &str = "clickhouse_backfill [START_YEAR] [--from YYYY-MM] [--through YYYY-MM] [--world ID]... [--replay]\n\
    Resume skips sealed past months; --replay repairs completed months too.\n\
    The current month is always replayed. Both month endpoints are inclusive.";

fn month(value: &str) -> anyhow::Result<chrono::NaiveDate> {
    anyhow::ensure!(value.len() == 7, "expected YYYY-MM, got {value}");
    Ok(chrono::NaiveDate::parse_from_str(
        &format!("{value}-01"),
        "%Y-%m-%d",
    )?)
}

fn options(args: impl IntoIterator<Item = String>) -> anyhow::Result<BackfillOptions> {
    let mut options = BackfillOptions::since(2022)?;
    let mut args = args.into_iter().peekable();
    if args.peek().is_some_and(|arg| !arg.starts_with('-')) {
        options = BackfillOptions::since(args.next().unwrap().parse()?)?;
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--replay" => options.replay = true,
            "--from" | "--through" | "--world" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("{arg} needs a value"))?;
                match arg.as_str() {
                    "--from" => options.from = month(&value)?,
                    "--through" => options.through = month(&value)?,
                    _ => options.worlds.push(value.parse()?),
                }
            }
            _ => anyhow::bail!("unknown argument {arg}\n{USAGE}"),
        }
    }
    Ok(options)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let options = options(args)?;
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let pg = UltrosDb::connect().await?;
    let ch = ClickHouseClient::from_env();
    ch.migrate().await?;
    let stats = backfill_sales_with_options(&pg, &ch, &options).await?;
    tracing::info!(?stats, "backfill complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<BackfillOptions> {
        options(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_targeted_repair_and_rejects_typos() {
        let options = parse(&[
            "--replay",
            "--from",
            "2026-08",
            "--through",
            "2026-09",
            "--world",
            "40",
            "--world",
            "41",
        ])
        .unwrap();
        assert!(options.replay);
        assert_eq!(options.from, month("2026-08").unwrap());
        assert_eq!(options.through, month("2026-09").unwrap());
        assert_eq!(options.worlds, [40, 41]);
        assert_eq!(parse(&["2025"]).unwrap().from, month("2025-01").unwrap());
        for bad in [
            &["--world"][..],
            &["--repla"],
            &["not-a-year"],
            &["--from", "2026-13"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
