use leptos::prelude::*;

use super::hover_card::{AccentHairline, HOVER_CARD_CHROME, HoverCard};

/// Plain-text tooltip. Thin wrapper over [`HoverCard`] — same public API as
/// the original standalone implementation.
#[component]
pub fn Tooltip<T>(
    #[prop(into)] tooltip_text: Signal<String>,
    #[prop(optional, into)] class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send + 'static,
{
    let disabled = Signal::derive(move || tooltip_text.with(|t| t.is_empty()));
    view! {
        <HoverCard
            disabled=disabled
            class=format!("inline-block {}", class.unwrap_or_default())
            content=move || {
                view! {
                    <div class=format!(
                        "{HOVER_CARD_CHROME} px-4 py-2 text-sm text-[color:var(--color-text)]",
                    )>
                        <AccentHairline />
                        {move || tooltip_text.get()}
                    </div>
                }
            }
            children=children
        />
    }
}
