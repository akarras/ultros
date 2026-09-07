use crate::components::icon::Icon;
use crate::components::related_items::item_source_counts;
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
pub fn SectionNav(#[prop(into)] item_id: Signal<i32>, children: Children) -> impl IntoView {
    let i18n = use_i18n();
    let sources = Memo::new(move |_| item_source_counts(item_id.get()));
    let nav_ref = NodeRef::<leptos::html::Nav>::new();
    let keep_focused_link_visible = move |_| {
        #[cfg(feature = "hydrate")]
        if let Some(nav) = nav_ref.get_untracked()
            && let Some(focused) = document().active_element()
            && nav.contains(Some(&focused))
        {
            // Some browsers reveal only part of a link when tabbing through a
            // horizontal scrollport. Keep its count visible too, without moving
            // the page vertically. Run after the browser's own focus scrolling.
            leptos::leptos_dom::helpers::request_animation_frame(move || {
                let bounds = nav.get_bounding_client_rect();
                let link = focused.get_bounding_client_rect();
                let delta = if link.left() < bounds.left() + 4.0 {
                    link.left() - bounds.left() - 4.0
                } else if link.right() > bounds.right() - 4.0 {
                    link.right() - bounds.right() + 4.0
                } else {
                    0.0
                };
                if delta != 0.0 {
                    nav.set_scroll_left(nav.scroll_left() + delta);
                }
            });
        }
    };
    let label = move |section: Section| match section {
        Section::Overview => t_string!(i18n, item_view_nav_overview).to_string(),
        Section::Listings => t_string!(i18n, item_view_nav_listings).to_string(),
        Section::History => t_string!(i18n, item_view_nav_history).to_string(),
        Section::Sources => t_string!(i18n, item_view_nav_sources).to_string(),
        Section::Related => t_string!(i18n, item_view_nav_related).to_string(),
    };
    view! {
        <div class="sticky top-0 z-20 backdrop-blur bg-[color:color-mix(in_srgb,var(--color-background)_88%,transparent)] border-b border-[color:var(--color-outline)]">
            <div class="w-full min-w-0 px-3 sm:px-4 py-1 flex items-center gap-2 sm:gap-3">
                <div class="max-w-24 sm:max-w-48 shrink-0 truncate">{children()}</div>
                <nav
                    node_ref=nav_ref
                    on:focusin=keep_focused_link_visible
                    data-item-section-nav=""
                    aria-label=move || t_string!(i18n, item_view_nav_aria).to_string()
                    class="min-w-0 flex-1 flex items-center gap-1 overflow-x-auto overscroll-x-contain"
                >
                    {Section::ALL
                        .iter()
                        .map(|&section| {
                            view! {
                                <a
                                    href=section.href()
                                    class="shrink-0 inline-flex min-h-11 items-center whitespace-nowrap rounded-md px-2.5 py-1 text-sm text-brand-300 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] hover:text-brand-100 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-brand-300"
                                >
                                    {label(section)}
                                </a>
                            }
                        })
                        .collect_view()}
                    {move || {
                        let counts = sources.get();
                        let links = [
                            (counts.craftable, "#crafting-recipes", t_string!(i18n, craftable).to_string(), t_string!(i18n, related_items_crafting_recipes_heading).to_string(), icondata::FaHammerSolid, "text-orange-300", "bg-orange-400/10"),
                            (counts.exchange, "#exchange-sources", t_string!(i18n, exchange).to_string(), t_string!(i18n, related_exchange_sources_title).to_string(), icondata::BsArrowLeftRight, "text-purple-300", "bg-purple-400/10"),
                            (counts.levequest, "#leve-sources", t_string!(i18n, item_view_nav_levequest).to_string(), t_string!(i18n, related_levequest_rewards_title).to_string(), icondata::FaScrollSolid, "text-pink-300", "bg-pink-400/10"),
                            (counts.vendor, "#vendor-sources", t_string!(i18n, item_view_nav_vendorable).to_string(), t_string!(i18n, related_vendor_sources_title).to_string(), icondata::FaShopSolid, "text-amber-300", "bg-amber-400/10"),
                        ];
                        links.into_iter().filter(|(count, ..)| *count > 0).enumerate().map(|(index, (count, href, label, description, icon, accent, chip))| {
                            let accessible_label = format!("{label}: {count} — {description}");
                            view! {
                                <div class="flex shrink-0 items-center">
                                    {(index == 0).then(|| view! {
                                        <span aria-hidden="true" class="mx-2 h-5 border-l border-[color:var(--color-outline)]"></span>
                                    })}
                                    <a
                                        href=href
                                        aria-label=accessible_label
                                        title=format!("{description}: {count}")
                                        class=format!("inline-flex min-h-11 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md px-2.5 py-1 text-sm transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-brand-300 {accent}")
                                    >
                                        <Icon icon=icon aria_hidden=true attr:class="shrink-0 text-sm" />
                                        <span>{label}</span>
                                        <span class=format!("min-w-5 rounded px-1 text-center text-xs leading-5 tabular-nums {chip}")>{count}</span>
                                    </a>
                                </div>
                            }
                        }).collect_view()
                    }}
                </nav>
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use leptos_i18n::context::init_i18n_context;

    fn render_nav(item_id: i32) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            provide_context(init_i18n_context::<Locale>());
            view! {
                <SectionNav item_id=Signal::derive(move || item_id)>
                    <span>"Gilgamesh"</span>
                </SectionNav>
            }
            .to_html()
        })
    }

    #[test]
    fn section_nav_preserves_section_links_without_sources() {
        let html = render_nav(-1);
        let mut last = 0;
        for section in Section::ALL {
            let offset = html.find(&format!("href=\"{}\"", section.href())).unwrap();
            assert!(offset >= last);
            last = offset;
        }
        for anchor in [
            "#crafting-recipes",
            "#exchange-sources",
            "#leve-sources",
            "#vendor-sources",
        ] {
            assert!(!html.contains(anchor));
        }
    }

    #[test]
    fn section_nav_renders_all_available_sources_in_ssr() {
        let data = crate::global_state::xiv_data::tracked_data();
        let id = data
            .recipes
            .values()
            .map(|r| r.item_result)
            .find(|id| {
                let counts = item_source_counts(*id);
                counts.craftable > 0
                    && (counts.vendor > 0 || counts.exchange > 0 || counts.levequest > 0)
            })
            .expect("game data contains craftable items with other acquisition sources");
        let counts = item_source_counts(id);
        let html = render_nav(id);
        let related = html.find("href=\"#related\"").unwrap();
        for (count, anchor) in [
            (counts.craftable, "#crafting-recipes"),
            (counts.exchange, "#exchange-sources"),
            (counts.levequest, "#leve-sources"),
            (counts.vendor, "#vendor-sources"),
        ] {
            let position = html.find(&format!("href=\"{anchor}\""));
            assert_eq!(position.is_some(), count > 0);
            if let Some(position) = position {
                assert!(position > related);
            }
        }
        assert!(html.contains("overflow-x-auto"));
        assert!(html.contains("min-h-11"));
        assert!(!html.contains("flex-wrap"));
    }
}
