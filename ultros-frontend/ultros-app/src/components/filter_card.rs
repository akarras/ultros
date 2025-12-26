use leptos::prelude::*;

#[component]
pub fn FilterCard<T>(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] description: Oco<'static, str>,
    children: TypedChildren<T>,
) -> impl IntoView
where
    T: IntoView,
{
    view! {
        <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100">
            <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">{title}</h3>
            <p class="mb-4 text-[color:var(--color-text-muted)]">{description}</p>
            {children.into_inner()().into_view()}
        </div>
    }
}
