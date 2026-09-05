use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::changelog::use_mark_changelog_seen;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;

use ultros_changelog::{CHANGELOG, ChangelogCategory, ChangelogEntry};

/// Preserve the crate's importance ordering within each day and category.
fn changelog_by_day() -> Vec<(&'static str, Vec<&'static ChangelogEntry>)> {
    let mut days: Vec<(&'static str, Vec<&'static ChangelogEntry>)> = Vec::new();
    for entry in CHANGELOG {
        match days.last_mut() {
            Some((date, entries)) if *date == entry.date => entries.push(entry),
            _ => days.push((entry.date, vec![entry])),
        }
    }
    days
}
#[component]
pub fn Changelog() -> impl IntoView {
    let i18n = use_i18n();
    use_mark_changelog_seen();

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
                <ol class="flex flex-col gap-6">
                    {changelog_by_day().into_iter().map(|(date, entries)| view! {
                        <li class="flex flex-col gap-3">
                            <h2 class="text-sm font-bold uppercase tracking-wide text-brand-300 tabular-nums">
                                <time datetime=date>{date}</time>
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
                                                    <h4 class="font-semibold text-[color:var(--brand-fg)]">{entry.title}</h4>
                                                    <p class="text-sm text-[color:var(--color-text-muted)]">{entry.blurb}</p>
                                                    {entry.link.map(|href| view! {
                                                        <AppLink
                                                            href=href
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
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_sections_preserve_every_entry_and_its_importance_order() {
        let days = changelog_by_day();
        assert!(days.windows(2).all(|pair| pair[0].0 > pair[1].0));
        let flattened: Vec<_> = days
            .iter()
            .flat_map(|(_, entries)| entries.iter().copied())
            .collect();
        assert_eq!(flattened, CHANGELOG.iter().collect::<Vec<_>>());
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
}
