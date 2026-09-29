## Summary

<!-- What does this PR change and why? Link any related issues (e.g. "Closes #123"). -->

## Screenshots

<!--
Required for any user-visible UI change. Include before/after where it helps.
Delete this section if the change has no visual impact.
-->

| Before | After |
| ------ | ----- |
|        |       |

## Validation / Test Steps

<!-- How was this verified? List the exact steps a reviewer can follow to reproduce. -->

1.
2.
3.

### Checks run

- [ ] `./check_ci.sh` passes (`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`)
- [ ] Relevant tests added or updated
- [ ] E2E smoke run where applicable (`./scripts/run_e2e.sh`)

## Checklist

- [ ] User-visible change has a changelog entry in `ultros-changelog/changes/`
- [ ] New user-facing strings use `leptos-i18n` and are added to **every** locale file
- [ ] No hardcoded user-facing strings in `ultros-frontend/ultros-app/`
- [ ] Docs / `AGENTS.md` updated if behaviour or workflow changed
