use leptos::prelude::*;

/// Horizontal filter bar primitive. Use as the top-of-route filter row
/// on analyzer pages; compose with [`ToolbarField`] and [`ToolbarPills`].
#[component]
pub fn Toolbar(children: Children) -> impl IntoView {
    view! {
        <div class="toolbar" role="toolbar">{children()}</div>
    }
    .into_any()
}

/// Labeled column inside a [`Toolbar`]. Renders the label above the slot
/// at small caps; slot is any input/select/control.
#[component]
pub fn ToolbarField(#[prop(into)] label: String, children: Children) -> impl IntoView {
    view! {
        <div class="toolbar-field">
            <span class="toolbar-field-label">{label}</span>
            {children()}
        </div>
    }
    .into_any()
}

/// Segmented pill group. Children should be `<button aria-pressed=...>`
/// elements that the caller controls.
#[component]
pub fn ToolbarPills(children: Children) -> impl IntoView {
    view! {
        <div class="toolbar-pills" role="group">{children()}</div>
    }
    .into_any()
}

/// Flex spacer to push subsequent toolbar children to the right edge.
#[component]
pub fn ToolbarSpacer() -> impl IntoView {
    view! { <div class="toolbar-spacer" /> }.into_any()
}
