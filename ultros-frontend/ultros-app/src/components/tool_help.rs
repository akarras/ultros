use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn ToolHeader(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] summary: Oco<'static, str>,
    #[prop(optional, into)] context: Option<Oco<'static, str>>,
    #[prop(into)] help_href: Oco<'static, str>,
    #[prop(into)] help_body: Oco<'static, str>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (is_open, set_is_open) = signal(false);
    let context_text = context.clone();
    let help_href_text = help_href.to_string();

    view! {
        <section class="panel px-4 py-3 sm:px-6 sm:py-4 rounded-2xl flex flex-col gap-3">
            <div class="flex flex-row items-center justify-between gap-3">
                <h1 class="text-xl sm:text-2xl font-bold text-[color:var(--brand-fg)]">
                    {title.clone()}
                </h1>
                <button
                    type="button"
                    class="btn-secondary self-center"
                    aria-expanded=move || if is_open() { "true" } else { "false" }
                    on:click=move |_| set_is_open.update(|open| *open = !*open)
                >
                    <Icon icon=i::BsInfoCircle width="1em" height="1em" />
                    <span>{move || if is_open() {
                        t_string!(i18n, tool_help_hide_info).to_string()
                    } else {
                        t_string!(i18n, tool_help_about_tool).to_string()
                    }}</span>
                </button>
            </div>
            <Show when=move || is_open()>
                <div class="rounded-xl border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] p-4 flex flex-col gap-3 max-w-3xl">
                    <p class="text-base text-[color:var(--color-text)] leading-relaxed">
                        {summary.clone()}
                    </p>
                    {
                        let context_text = context_text.clone();
                        move || context_text.clone().map(|context| view! {
                            <p class="text-sm text-[color:var(--color-text-muted)]">{context}</p>
                        })
                    }
                    <p class="text-sm leading-relaxed text-[color:var(--color-text)]">
                        {help_body.clone()}
                    </p>
                    <A href=help_href_text.clone() attr:class="text-sm text-brand-300 hover:text-[color:var(--brand-fg)] font-semibold inline-flex items-center gap-2">
                        {t!(i18n, tool_help_open_full_help)}
                        <Icon icon=i::FaArrowRightSolid width="0.85em" height="0.85em" />
                    </A>
                </div>
            </Show>
        </section>
    }
}

#[component]
pub fn CalculationSummary(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] formula: Oco<'static, str>,
    #[prop(into)] details: Oco<'static, str>,
) -> impl IntoView {
    view! {
        <aside class="panel p-4 rounded-xl flex flex-col gap-2 border border-[color:var(--color-outline)]">
            <div class="flex items-center gap-2 text-[color:var(--brand-fg)] font-semibold">
                <Icon icon=i::AiCalculatorOutlined width="1.1em" height="1.1em" />
                <span>{title}</span>
            </div>
            <code class="text-sm text-brand-300 whitespace-normal break-words">{formula}</code>
            <p class="text-sm text-[color:var(--color-text-muted)] leading-relaxed">{details}</p>
        </aside>
    }
}

#[component]
pub fn AssumptionBadge(#[prop(into)] text: Oco<'static, str>) -> impl IntoView {
    view! {
        <span class="inline-flex items-center gap-1 rounded-full border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] px-3 py-1 text-xs font-medium text-[color:var(--color-text)]">
            <Icon icon=i::BsCheck2Circle width="0.9em" height="0.9em" />
            {text}
        </span>
    }
}

#[component]
pub fn ConfidenceBadge(total_sales: usize, daily_sales: f32) -> impl IntoView {
    let i18n = use_i18n();
    let (label, class) = if total_sales >= 20 && daily_sales >= 1.0 {
        (
            t_string!(i18n, confidence_high).to_string(),
            "text-emerald-300",
        )
    } else if total_sales >= 5 {
        (
            t_string!(i18n, confidence_medium).to_string(),
            "text-amber-300",
        )
    } else {
        (
            t_string!(i18n, confidence_low_data).to_string(),
            "text-red-300",
        )
    };

    view! {
        <span class=format!("inline-flex items-center justify-end rounded-full border border-[color:var(--color-outline)] px-2 py-1 text-xs font-semibold {class}")>
            {label}
        </span>
    }
}

#[component]
#[allow(dead_code)]
pub fn ActionableEmptyState(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] body: Oco<'static, str>,
    #[prop(optional, into)] action_href: Option<Oco<'static, str>>,
    #[prop(optional, into)] action_label: Option<Oco<'static, str>>,
) -> impl IntoView {
    view! {
        <div class="panel p-6 rounded-2xl text-center flex flex-col items-center gap-3">
            <div class="text-brand-300">
                <Icon icon=i::BsInfoCircle width="2em" height="2em" />
            </div>
            <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{title}</h2>
            <p class="max-w-prose text-sm text-[color:var(--color-text-muted)] leading-relaxed">{body}</p>
            {move || {
                action_href.clone().zip(action_label.clone()).map(|(href, label)| view! {
                    <A href=href.to_string() attr:class="btn-primary mt-2">
                        {label}
                    </A>
                })
            }}
        </div>
    }
}

#[component]
#[allow(dead_code)]
pub fn ResultBreakdownDisclosure<T>(
    #[prop(into)] title: Oco<'static, str>,
    children: TypedChildren<T>,
) -> impl IntoView
where
    T: IntoView,
{
    view! {
        <details class="text-xs text-[color:var(--color-text-muted)]">
            <summary class="cursor-pointer text-brand-300 hover:text-[color:var(--brand-fg)]">
                {title}
            </summary>
            <div class="mt-2">{children.into_inner()().into_view()}</div>
        </details>
    }
}
