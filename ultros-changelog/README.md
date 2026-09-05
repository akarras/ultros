# Ultros changelog

Add one file per change in `changes/`, named `YYYY-MM-DD-description.json`.
For example, `2026-09-04-clearer-changelog.json`:

```json
{
  "category": "improvements",
  "importance": "medium",
  "title": "A changelog that's easier to scan",
  "blurb": "Each day's changes are grouped into features, improvements, and bug fixes.",
  "link": "/changelog"
}
```

The filename supplies the ship date. Use a unique lowercase description with
hyphens. Each change adds its own file; never append to a shared daily file.

- `category`: `features`, `improvements`, or `bug_fixes`.
- `importance`: `high` for major changes, `medium` for ordinary changes,
  or `low` for minor polish. This field is required.
- `title` and `blurb`: concise, player-facing plain text.
- `link`: optional internal app route.

`build.rs` validates these files and generates a static
`CHANGELOG: &[ChangelogEntry]`, newest day first, then high/medium/low importance,
then filename for stable ties. The crate has no runtime dependencies.
The app groups this list into daily category sections while preserving priority
within each section. Generated Rust stays in Cargo's `OUT_DIR`.

Run `cargo test -p ultros-changelog` for this small crate's tests and
`./check_ci.sh` for the repository's required checks.
