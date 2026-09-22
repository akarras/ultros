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
- `labs`: optional, `true` when the change only exists behind a Labs toggle.
  The page shows a Labs badge next to it, and it never turns on the sidebar's
  what's-new dot, since most players cannot see the change yet. Use the
  badge instead of writing "(Labs)" in the title.

`build.rs` validates these files and generates two things into Cargo's
`OUT_DIR`:

- `CHANGELOG: &[ChangelogEntry]`, newest day first, then high/medium/low
  importance, then filename for stable ties. It is compiled in only with the
  `history` feature, which is on by default — **the wasm client turns it off**
  (`default-features = false`) and fetches `/api/v1/changelog` instead, so a
  couple of hundred entries of prose stay out of the bundle for one of the
  least-visited routes. The server crate owns that endpoint, and the page's
  blocking SSR resource still renders the whole list into the initial HTML.
- `LATEST_CHANGELOG_DATE` and `LATEST_ANNOUNCED_CHANGELOG_DATE`, the two `&str`
  consts the sidebar's what's-new dot compares against. These are all the
  client gets. Adding a Labs-only day moves the first and not the second.

`ChangelogEntry` is the wire type as well as the compiled one — its text is
`Cow<'static, str>`, borrowed from the binary on the server and owned on the
client. The app groups the list into daily category sections while preserving
priority within each section.

Run `cargo test -p ultros-changelog` for this small crate's tests and
`./check_ci.sh` for the repository's required checks.
