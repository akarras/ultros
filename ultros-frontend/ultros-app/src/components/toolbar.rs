// WHY THIS ATTRIBUTE EXISTS (remove when Track 3 ships its first analyzer migration):
//
// `#[component]` generates Props structs for each component:
//   - `ToolbarProps { children: Children }`
//   - `ToolbarFieldProps { label: String, children: Children }`
//   - `ToolbarPillsProps { children: Children }`
//
// The fields in those structs are never read by any code outside the macro
// expansion — they are dead until Track 3 migrates analyzer routes to consume
// these primitives (e.g. replacing `FilterCard` with `<Toolbar>/<ToolbarField>`).
//
// `#[expect(dead_code)]` cannot be used here because:
//   1. The generated structs have no user-visible annotation site.
//   2. Each component *function* is considered live by the compiler (the macro
//      registers it), so placing `#[expect]` on the function produces an
//      `unfulfilled_lint_expectations` error.
//   3. `#[allow]` placed before OR after `#[component]`, or on the parameters,
//      is NOT propagated to the generated structs by the Leptos proc-macro.
//
// The module-level inner attribute is therefore the only surgical suppression
// available.  It covers both lib and lib-test builds because `ToolbarProps` /
// `ToolbarFieldProps` / `ToolbarPillsProps` are equally unreachable in both
// until Track 3 wires in the first analyzer route migration.
//
// Confirmed with: `cargo clippy -p ultros-app --all-targets -- -D warnings`
// Exact errors without this attribute:
//   error: field `children` is never read
//     --> ultros-frontend/ultros-app/src/components/toolbar.rs (ToolbarProps)
//   error: fields `label` and `children` are never read
//     --> ultros-frontend/ultros-app/src/components/toolbar.rs (ToolbarFieldProps)
//   error: field `children` is never read
//     --> ultros-frontend/ultros-app/src/components/toolbar.rs (ToolbarPillsProps)
//
// TODO(Track 3): Delete this attribute and this entire comment block once the
// first analyzer route adopts <Toolbar> and these Props structs are live.
#![allow(dead_code)]

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
