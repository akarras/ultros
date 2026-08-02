use leptos::prelude::*;
use xiv_gen::{ItemId, ItemUiCategoryId};

use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;

use super::hover_card::{AccentHairline, HOVER_CARD_CHROME, HoverCard};
use super::item_icon::{IconSize, ItemIcon};
use super::stats_display::ItemStats;
use super::ui_text::UIText;

/// Sustained-hover delay before the card opens, so sweeping the cursor across
/// item tables doesn't strobe cards.
const OPEN_DELAY_MS: u32 = 300;

/// Wraps any item surface (row, icon, link) with a hover card showing the
/// item's icon, name, category, item level, stats, and description. All data
/// comes synchronously from `tracked_data()` — no fetches. Unknown ids render
/// the children with hover disabled.
#[component]
pub fn ItemTooltip<T>(
    #[prop(into)] item_id: Signal<i32>,
    /// Classes for the anchor wrapper div (use to preserve the layout the
    /// wrapped content expects, e.g. flex row classes).
    #[prop(optional, into)]
    class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send + 'static,
{
    let i18n = use_i18n();
    let disabled =
        Signal::derive(move || !tracked_data().items.contains_key(&ItemId(item_id.get())));
    let content = move || {
        let data = tracked_data();
        let Some(item) = data.items.get(&ItemId(item_id.get_untracked())) else {
            return ().into_any();
        };
        let category = data
            .item_ui_categorys
            .get(&ItemUiCategoryId(item.item_ui_category))
            .map(|category| category.name.as_str());
        view! {
            <div class=format!("{HOVER_CARD_CHROME} w-max max-w-md p-4 flex flex-col gap-3")>
                <AccentHairline />
                <div class="flex items-center gap-3">
                    <div class="relative shrink-0">
                        // Soft palette-tinted bloom behind the icon.
                        <div class="absolute -inset-2 rounded-full bg-[radial-gradient(circle,var(--accent-glow),transparent_70%)]"></div>
                        <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium />
                    </div>
                    <div class="flex flex-col min-w-0 flex-1">
                        <span class="font-bold text-[color:var(--color-text)] leading-tight">
                            {item.name.as_str()}
                        </span>
                        {category
                            .map(|name| {
                                view! { <span class="text-sm text-brand-300">{name}</span> }
                            })}
                    </div>
                    <div class="flex items-center gap-1.5 shrink-0 self-start">
                        <span class="text-brand-300 font-medium tracking-wide text-xs uppercase">
                            {t_string!(i18n, item_level).to_string()}
                        </span>
                        <span class="text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-400/50">
                            {item.level_item}
                        </span>
                    </div>
                </div>
                <ItemStats item_id=item.key_id />
                {(!item.description.is_empty())
                    .then(|| {
                        view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] line-clamp-3">
                                <UIText text=item.description.as_str().to_string() />
                            </div>
                        }
                    })}
            </div>
        }
        .into_any()
    };
    view! {
        <HoverCard
            content=content
            disabled=disabled
            open_delay_ms=OPEN_DELAY_MS
            class=class.unwrap_or_default()
            children=children
        />
    }
}
