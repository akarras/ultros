use leptos::prelude::*;
use leptos_router::hooks::use_location;
use leptos_router::location::Location;

/// A button that sets the query property to the given value
#[component]
pub fn QueryButton<T>(
    /// query key that we filter against.
    #[prop(into)]
    key: Oco<'static, str>,
    /// query value that is set when we check this box
    #[prop(into)]
    value: Signal<String>,
    /// default state classes
    #[prop(into)]
    class: Signal<String>,
    /// classes that will replace the main classes when this is active
    #[prop(into)]
    active_classes: Oco<'static, str>,
    #[prop(optional)] default: bool,
    /// List of other query names that should be removed when preparing this query
    #[prop(optional)]
    remove_queries: &'static [&'static str],
    children: TypedChildren<T>,
) -> impl IntoView + 'static
where
    T: IntoView + 'static,
{
    let Location {
        pathname, query, ..
    } = use_location();
    let key_1 = key.clone();
    let is_active = Signal::derive(move || {
        query.with(|q| {
            let name = key_1.as_str();
            let query_val = q.get_str(name);
            value.with(|val| val.as_str() == query_val.unwrap_or_default())
                || (default == true && query_val.is_none())
        })
    });
    view! {
        <a
            class=move || if is_active() { active_classes.to_string() } else { class.get() }
            href=move || {
                let mut query = query();
                for remove in remove_queries {
                    query.remove(remove);
                }
                let _ = query.insert(key.to_string(), value.get());
                format!("{}{}", pathname(), query.to_query_string())
            }
        >
            {children.into_inner()().into_view()}
        </a>
    }
    .into_any()
}

