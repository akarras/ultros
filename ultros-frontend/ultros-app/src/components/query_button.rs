use leptos::*;
use leptos_router::*;

/// A button that sets the query property to the given value
#[component]
pub fn QueryButton(
    #[prop(into)] query_name: TextProp,
    /// default state classes
    #[prop(into)]
    class: TextProp,
    /// classes that will replace the main classes when this is active
    #[prop(into)]
    active_classes: TextProp,
    #[prop(into)] value: TextProp,
    #[prop(optional)] default: bool,
    /// List of other query names that should be removed when preparing this query
    #[prop(optional)]
    remove_queries: &'static [&'static str],
    children: Box<dyn Fn() -> Fragment>,
) -> impl IntoView {
    let Location {
        pathname, query, ..
    } = use_location();
    let query_1 = query_name.clone();
    let value_1 = value.clone();
    let is_active = move || {
        let query_name = query_1.get();
        let value = value_1.get();
        query.with(|q| {
            let query_val = q.get(&query_name).as_ref().map(|s| s.as_str());
            query_val.unwrap_or_default() == &value || (default == true && query_val.is_none())
        })
    };
    view! { <a class=move || if is_active() { active_classes.get() } else { class.get() }.to_string() href=move || {
        let mut query = query();
        for remove in remove_queries {
            query.remove(remove);
        }
        let _ = query.insert(query_name.get().to_string(), value.get().to_string());
        format!("{}{}", pathname(), query.to_query_string())
    }>{children}</a> }
}
