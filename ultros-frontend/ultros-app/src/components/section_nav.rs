use crate::i18n::{t_string, use_i18n};
use crate::routes::item_view_sections::Section;
use leptos::prelude::*;

/// Slim sticky bar for the item view: scope label on the left, in-page jump
/// nav on the right.
///
/// Rendered below the full world menu in the DOM. `position: sticky` engages
/// only when the bar reaches the top of the viewport, so the world pills — ~30
/// crawlable links to sibling worlds — scroll away naturally and this takes
/// over without a scroll listener.
#[component]
pub fn SectionNav(children: Children) -> impl IntoView {
    let i18n = use_i18n();
    let label = move |section: Section| match section {
        Section::Overview => t_string!(i18n, item_view_nav_overview).to_string(),
        Section::Listings => t_string!(i18n, item_view_nav_listings).to_string(),
        Section::History => t_string!(i18n, item_view_nav_history).to_string(),
        Section::Sources => t_string!(i18n, item_view_nav_sources).to_string(),
        Section::Related => t_string!(i18n, item_view_nav_related).to_string(),
    };
    view! {
        <div class="sticky top-0 z-20 backdrop-blur bg-[color:color-mix(in_srgb,var(--color-background)_88%,transparent)] border-b border-[color:var(--color-outline)]">
            <div class="w-full px-3 sm:px-4 py-2 flex items-center gap-3 flex-wrap">
                {children()}
                <nav
                    aria-label=move || t_string!(i18n, item_view_nav_aria).to_string()
                    class="flex items-center gap-1 overflow-x-auto"
                >
                    {Section::ALL
                        .iter()
                        .map(|&section| {
                            view! {
                                <a
                                    href=section.href()
                                    class="whitespace-nowrap rounded-md px-2.5 py-1 text-sm text-brand-300 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] hover:text-brand-100"
                                >
                                    {label(section)}
                                </a>
                            }
                        })
                        .collect_view()}
                </nav>
            </div>
        </div>
    }
    .into_any()
}
