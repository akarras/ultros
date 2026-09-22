//! The changelog page.
//!
//! The history is fetched rather than compiled in: `ultros-changelog` keeps
//! the whole table behind a server-only feature, so the wasm bundle carries
//! two dates instead of a couple of hundred entries of prose.
//!
//! The route is registered `SsrMode::InOrder` with a blocking resource, so a
//! cold load still puts the whole list in the initial HTML in document
//! order — the same thing a crawler used to get from a static array.

use crate::api::get_changelog;
use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::skeleton::BoxSkeleton;
use crate::global_state::changelog::use_mark_changelog_seen;
use crate::i18n::*;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;

use ultros_changelog::{ChangelogCategory, ChangelogEntry};

/// Group the flat, server-sorted list into days, preserving the importance
/// ordering the server already applied within each day and category.
fn changelog_by_day(entries: &[ChangelogEntry]) -> Vec<(&str, Vec<&ChangelogEntry>)> {
    let mut days: Vec<(&str, Vec<&ChangelogEntry>)> = Vec::new();
    for entry in entries {
        match days.last_mut() {
            Some((date, entries)) if *date == entry.date => entries.push(entry),
            _ => days.push((&entry.date, vec![entry])),
        }
    }
    days
}

#[component]
pub fn Changelog() -> impl IntoView {
    let i18n = use_i18n();
    use_mark_changelog_seen();
    // Blocking: this *is* the page, so there is nothing useful to stream
    // around it, and the server answers from a static table in memory. See
    // the route's `SsrMode::InOrder` in `lib.rs`.
    let changelog = Resource::new_blocking(|| (), |()| get_changelog());

    view! {
        <MetaTitle title=t_string!(i18n, changelog_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, changelog_meta_desc).to_string() />
        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-4xl flex flex-col gap-6">
                <section class="panel p-6 sm:p-8 rounded-2xl">
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-3">
                        {t!(i18n, changelog_page_heading)}
                    </h1>
                    <p class="text-lg text-[color:var(--color-text)] max-w-2xl">
                        {t!(i18n, changelog_intro)}
                    </p>
                </section>
                <Suspense fallback=move || view! { <BoxSkeleton rows=8 /> }>
                    {move || {
                        changelog
                            .get()
                            .map(|result| match result {
                                Ok(entries) => {
                                    Either::Left(
                                        view! {
                                            <ol class="flex flex-col gap-6">
                                                {changelog_by_day(&entries).into_iter().map(|(date, entries)| view! {
                                                    <li class="flex flex-col gap-3">
                                                        <h2 class="text-sm font-bold uppercase tracking-wide text-brand-300 tabular-nums">
                                                            <time datetime=date.to_string()>{date.to_string()}</time>
                                                        </h2>
                                                        <div class="panel rounded-xl divide-y divide-[color:var(--color-outline)]">
                                                            {ChangelogCategory::ALL.into_iter().filter(|category| entries.iter().any(|entry| entry.category == *category)).map(|category| view! {
                                                                <section class="p-5">
                                                                    <h3 class="text-lg font-bold text-[color:var(--brand-fg)] mb-4">
                                                                        {match category {
                                                                            ChangelogCategory::Features => t!(i18n, changelog_features).into_any(),
                                                                            ChangelogCategory::Improvements => t!(i18n, changelog_improvements).into_any(),
                                                                            ChangelogCategory::BugFixes => t!(i18n, changelog_bug_fixes).into_any(),
                                                                        }}
                                                                    </h3>
                                                                    <ul class="flex flex-col gap-5">
                                                                        {entries.iter().filter(|entry| entry.category == category).map(|entry| view! {
                                                                            <li class="flex flex-col gap-2">
                                                                                <h4 class="font-semibold text-[color:var(--brand-fg)] flex flex-wrap items-center gap-2">
                                                                                    <span>{entry.title.to_string()}</span>
                                                                                    {entry.labs.then(|| view! {
                                                                                        <span
                                                                                            class="text-xs font-semibold uppercase tracking-wide rounded-full px-2 py-0.5 border border-brand-300/60 text-brand-300"
                                                                                            title=t_string!(i18n, changelog_labs_hint)
                                                                                        >
                                                                                            {t!(i18n, labs_title)}
                                                                                        </span>
                                                                                    })}
                                                                                </h4>
                                                                                <p class="text-sm text-[color:var(--color-text-muted)]">{entry.blurb.to_string()}</p>
                                                                                {entry.link.as_ref().map(|href| view! {
                                                                                    <AppLink
                                                                                        href=href.to_string()
                                                                                        attr:class="text-sm text-brand-300 hover:text-[color:var(--brand-fg)] inline-flex items-center gap-1.5 self-start"
                                                                                    >
                                                                                        {t!(i18n, changelog_try_it)}
                                                                                        <Icon icon=i::FaArrowRightSolid width="0.8em" height="0.8em" />
                                                                                    </AppLink>
                                                                                })}
                                                                            </li>
                                                                        }).collect_view()}
                                                                    </ul>
                                                                </section>
                                                            }).collect_view()}
                                                        </div>
                                                    </li>
                                                }).collect_view()}
                                            </ol>
                                        },
                                    )
                                }
                                Err(error) => {
                                    Either::Right(
                                        view! {
                                            <div class="alert alert-error">
                                                {move || t!(i18n, changelog_error_loading, error = error.to_string())}
                                            </div>
                                        },
                                    )
                                }
                            })
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use ultros_changelog::ChangelogImportance;

    fn entry(
        date: &'static str,
        category: ChangelogCategory,
        importance: ChangelogImportance,
        title: &'static str,
    ) -> ChangelogEntry {
        ChangelogEntry {
            date: Cow::Borrowed(date),
            category,
            importance,
            title: Cow::Borrowed(title),
            blurb: Cow::Borrowed("A player-facing change."),
            link: None,
            labs: false,
        }
    }

    /// Grouping is pure bookkeeping over the order the server sent: every
    /// entry lands in exactly one day, days stay newest-first, and importance
    /// order survives inside each category section the page renders.
    #[test]
    fn daily_sections_preserve_every_entry_and_its_importance_order() {
        use ChangelogCategory::*;
        use ChangelogImportance::*;
        let feed = vec![
            entry("2026-09-04", Features, High, "Newest"),
            entry("2026-09-04", BugFixes, Medium, "Same day, other section"),
            entry("2026-09-04", Features, Low, "Same day, same section"),
            entry("2026-09-01", Improvements, High, "Older day"),
        ];
        let days = changelog_by_day(&feed);
        assert_eq!(days.len(), 2);
        assert!(days.windows(2).all(|pair| pair[0].0 > pair[1].0));
        let flattened: Vec<_> = days
            .iter()
            .flat_map(|(_, entries)| entries.iter().copied())
            .collect();
        assert_eq!(flattened, feed.iter().collect::<Vec<_>>());
        for (date, entries) in days {
            assert!(entries.iter().all(|entry| entry.date == date));
            for category in ChangelogCategory::ALL {
                let section: Vec<_> = entries
                    .iter()
                    .filter(|entry| entry.category == category)
                    .collect();
                assert!(
                    section
                        .windows(2)
                        .all(|pair| pair[0].importance <= pair[1].importance)
                );
            }
        }
    }

    /// An empty history renders an empty list rather than a phantom day.
    #[test]
    fn no_entries_means_no_days() {
        assert!(changelog_by_day(&[]).is_empty());
    }
}
