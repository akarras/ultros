use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use leptos_i18n::I18nContext;

#[derive(Clone)]
pub struct ToolCalculation {
    title: String,
    formula: String,
    details: String,
}

impl ToolCalculation {
    pub fn new(
        title: impl Into<String>,
        formula: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            formula: formula.into(),
            details: details.into(),
        }
    }
}

/// Slim single-row tool header: the tool's `h1`, an icon-only About toggle,
/// and an optional right-aligned slot for the page's controls (world picker,
/// small filters). Pages pass controls as children so the title and controls
/// share one row instead of stacking two full-width bars. Must stay outside
/// any Suspense/Transition boundary so the controls survive loading states.
#[component]
pub fn ToolHeader(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] summary: Oco<'static, str>,
    #[prop(optional, into)] context: Option<Oco<'static, str>>,
    /// Link to a full help page. Omit on pages that have no dedicated help
    /// doc (lists, alerts, settings) — the "open full help" link is only
    /// rendered when both this and `help_body` are set.
    #[prop(optional, into)]
    help_href: Option<Oco<'static, str>>,
    /// Extra detail shown below the summary when the info panel is expanded.
    /// Optional for the same reason as `help_href`.
    #[prop(optional, into)]
    help_body: Option<Oco<'static, str>>,
    /// Optional calculation model and assumptions, shown only inside the
    /// expanded info panel so analyzer results stay near the top of the page.
    #[prop(optional, into)]
    calculation: Option<ToolCalculation>,
    #[prop(optional)] assumptions: Vec<String>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (is_open, set_is_open) = signal(false);
    let context_text = context.clone();
    let calculation_details = calculation.clone();
    let assumption_details = assumptions.clone();
    let help_link = help_href
        .clone()
        .zip(help_body.clone())
        .map(|(href, body)| (href.to_string(), body));
    let toggle_label = move || {
        if is_open() {
            t_string!(i18n, tool_help_hide_info).to_string()
        } else {
            t_string!(i18n, tool_help_about_tool).to_string()
        }
    };

    view! {
        <section class="flex flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
                <h1 class="text-lg sm:text-xl font-bold text-[color:var(--brand-fg)]">
                    {title.clone()}
                </h1>
                <button
                    type="button"
                    class="btn-ghost p-2 rounded-full"
                    title=toggle_label
                    aria-label=toggle_label
                    aria-expanded=move || if is_open() { "true" } else { "false" }
                    on:click=move |_| set_is_open.update(|open| *open = !*open)
                >
                    <Icon icon=i::BsInfoCircle width="1.1em" height="1.1em" />
                </button>
                {children.map(|children| {
                    view! {
                        <div class="ms-auto flex flex-wrap items-center gap-3">
                            {children()}
                        </div>
                    }
                })}
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
                    {
                        let help_link = help_link.clone();
                        move || help_link.clone().map(|(href, body)| view! {
                            <p class="text-sm leading-relaxed text-[color:var(--color-text)]">
                                {body}
                            </p>
                            <AppLink href=href attr:class="text-sm text-brand-300 hover:text-[color:var(--brand-fg)] font-semibold inline-flex items-center gap-2">
                                {t!(i18n, tool_help_open_full_help)}
                                <Icon icon=i::FaArrowRightSolid width="0.85em" height="0.85em" />
                            </AppLink>
                        })
                    }
                    {
                        let calculation_details = calculation_details.clone();
                        move || calculation_details.clone().map(|calculation| view! {
                            <div class="border-t border-[color:var(--color-outline)] pt-3 flex flex-col gap-2">
                                <div class="flex items-center gap-2 text-[color:var(--brand-fg)] font-semibold">
                                    <Icon icon=i::AiCalculatorOutlined width="1.1em" height="1.1em" />
                                    <span>{calculation.title}</span>
                                </div>
                                <code class="text-sm text-brand-300 whitespace-normal break-words">
                                    {calculation.formula}
                                </code>
                                <p class="text-sm text-[color:var(--color-text-muted)] leading-relaxed">
                                    {calculation.details}
                                </p>
                            </div>
                        })
                    }
                    {
                        let assumption_details = assumption_details.clone();
                        move || (!assumption_details.is_empty()).then(|| view! {
                            <div class="flex flex-wrap gap-2">
                                {assumption_details.clone().into_iter().map(|assumption| view! {
                                    <span class="inline-flex items-center gap-1 rounded-full border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] px-3 py-1 text-xs font-medium text-[color:var(--color-text)]">
                                        <Icon icon=i::BsCheck2Circle width="0.9em" height="0.9em" />
                                        {assumption}
                                    </span>
                                }).collect_view()}
                            </div>
                        })
                    }
                </div>
            </Show>
        </section>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolConfidenceLevel {
    High,
    Medium,
    LowData,
}

impl ToolConfidenceLevel {
    pub fn css_class(&self) -> &'static str {
        match self {
            Self::High => "text-emerald-300",
            Self::Medium => "text-amber-300",
            Self::LowData => "text-red-300",
        }
    }

    pub fn get_text(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        match self {
            Self::High => t_string!(i18n, confidence_high).to_string(),
            Self::Medium => t_string!(i18n, confidence_medium).to_string(),
            Self::LowData => t_string!(i18n, confidence_low_data).to_string(),
        }
    }
}

pub fn get_tool_confidence_level(total_sales: usize, daily_sales: f32) -> ToolConfidenceLevel {
    if total_sales >= 20 && daily_sales >= 1.0 {
        ToolConfidenceLevel::High
    } else if total_sales >= 5 {
        ToolConfidenceLevel::Medium
    } else {
        ToolConfidenceLevel::LowData
    }
}

#[component]
pub fn ConfidenceBadge(total_sales: usize, daily_sales: f32) -> impl IntoView {
    let i18n = use_i18n();
    let level = get_tool_confidence_level(total_sales, daily_sales);
    let label = level.get_text(i18n);
    let class = level.css_class();

    view! {
        <span class=format!("inline-flex items-center justify-end rounded-full border border-[color:var(--color-outline)] px-2 py-1 text-xs font-semibold {class}")>
            {label}
        </span>
    }
}

#[component]
pub fn ActionableEmptyState(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] body: Oco<'static, str>,
    #[prop(optional, into)] action_href: Option<Oco<'static, str>>,
    #[prop(optional, into)] action_label: Option<Oco<'static, str>>,
    /// In-page action rendered as a button. Takes precedence over
    /// `action_href` when both are provided — a callback caller wants the
    /// current page mutated (e.g. filters cleared), not a navigation.
    #[prop(optional, into)]
    on_action: Option<Callback<()>>,
    /// Render the primary action as a plain `<a rel="external">` rather than a
    /// client-side `<AppLink>`. Needed for server routes (`/login`) that the leptos
    /// router must not try to handle.
    #[prop(optional)]
    action_external: bool,
    #[prop(optional, into)] secondary_action_href: Option<Oco<'static, str>>,
    #[prop(optional, into)] secondary_action_label: Option<Oco<'static, str>>,
    #[prop(optional)] secondary_action_external: bool,
) -> impl IntoView {
    view! {
        <div class="panel p-6 rounded-2xl text-center flex flex-col items-center gap-3">
            <div class="text-brand-300">
                <Icon icon=i::BsInfoCircle width="2em" height="2em" />
            </div>
            <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{title}</h2>
            <p class="max-w-prose text-sm text-[color:var(--color-text-muted)] leading-relaxed">{body}</p>
            <div class="flex flex-wrap gap-4 mt-2 justify-center">
                {move || {
                    let label = action_label.clone()?;
                    if let Some(on_action) = on_action {
                        return Some(
                            view! {
                                <button
                                    type="button"
                                    class="btn-primary"
                                    on:click=move |_| on_action.run(())
                                >
                                    {label}
                                </button>
                            }
                                .into_any(),
                        );
                    }
                    let href = action_href.clone()?;
                    Some(
                        if action_external {
                            view! {
                                <a href=href.to_string() rel="external" class="btn-primary">
                                    {label}
                                </a>
                            }
                                .into_any()
                        } else {
                            view! {
                                <AppLink href=href.to_string() attr:class="btn-primary">
                                    {label}
                                </AppLink>
                            }
                                .into_any()
                        },
                    )
                }}
                {move || {
                    let label = secondary_action_label.clone()?;
                    let href = secondary_action_href.clone()?;
                    Some(
                        if secondary_action_external {
                            view! {
                                <a href=href.to_string() rel="external" class="btn-secondary">
                                    {label}
                                </a>
                            }
                                .into_any()
                        } else {
                            view! {
                                <AppLink href=href.to_string() attr:class="btn-secondary">
                                    {label}
                                </AppLink>
                            }
                                .into_any()
                        },
                    )
                }}
            </div>
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_confidence_level_logic() {
        // High confidence: total_sales >= 20 AND daily_sales >= 1.0
        assert_eq!(
            get_tool_confidence_level(20, 1.0),
            ToolConfidenceLevel::High
        );
        assert_eq!(
            get_tool_confidence_level(100, 5.0),
            ToolConfidenceLevel::High
        );

        // Medium confidence (misses daily_sales but >= 20 sales)
        assert_eq!(
            get_tool_confidence_level(20, 0.9),
            ToolConfidenceLevel::Medium
        );

        // Medium confidence (misses total_sales but >= 5 sales)
        assert_eq!(
            get_tool_confidence_level(19, 1.0),
            ToolConfidenceLevel::Medium
        );
        assert_eq!(
            get_tool_confidence_level(5, 0.0),
            ToolConfidenceLevel::Medium
        );

        // Low data: total_sales < 5
        assert_eq!(
            get_tool_confidence_level(4, 100.0),
            ToolConfidenceLevel::LowData
        );
        assert_eq!(
            get_tool_confidence_level(0, 0.0),
            ToolConfidenceLevel::LowData
        );
    }

    #[test]
    fn test_tool_confidence_level_css_class() {
        assert_eq!(ToolConfidenceLevel::High.css_class(), "text-emerald-300");
        assert_eq!(ToolConfidenceLevel::Medium.css_class(), "text-amber-300");
        assert_eq!(ToolConfidenceLevel::LowData.css_class(), "text-red-300");
    }
}
