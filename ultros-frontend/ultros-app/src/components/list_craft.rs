//! Live multi-recipe crafting project view shared by account and device lists.

use std::collections::BTreeMap;

use icondata as i;
use leptos::prelude::*;
use ultros_calc::recipe_planner::StepReadiness;
use ultros_list_doc::CraftMode;

use crate::components::icon::Icon;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::i18n::*;
use crate::list_doc::craft_project::{
    CraftProjectState, CraftStepView, CraftTargetView, format_gil,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum StepGrouping {
    #[default]
    StepOrder,
    ByJob,
}

fn readiness_badge(
    i18n: leptos_i18n::I18nContext<Locale, I18nKeys>,
    readiness: StepReadiness,
) -> (&'static str, String) {
    match readiness {
        StepReadiness::Done => (
            "craft-pill craft-pill-done",
            t_string!(i18n, craft_readiness_done).to_string(),
        ),
        StepReadiness::Ready { craftable_now } => (
            "craft-pill craft-pill-ready",
            t_string!(i18n, craft_readiness_ready, count = craftable_now).to_string(),
        ),
        StepReadiness::NeedMaterials { missing_kinds } => (
            "craft-pill craft-pill-warn",
            t_string!(i18n, craft_readiness_need_mats, count = missing_kinds).to_string(),
        ),
        StepReadiness::Buying => (
            "craft-pill craft-pill-buy",
            t_string!(i18n, craft_readiness_buying).to_string(),
        ),
    }
}

fn step_readiness_attr(readiness: StepReadiness) -> &'static str {
    match readiness {
        StepReadiness::Done => "done",
        StepReadiness::Ready { .. } => "ready",
        StepReadiness::NeedMaterials { .. } => "missing",
        StepReadiness::Buying => "buying",
    }
}

#[component]
pub fn ListCraft(
    project: Signal<CraftProjectState>,
    can_write: Signal<bool>,
    on_set_target_count: Callback<(i32, i64)>,
    on_add_target_completed: Callback<(i32, i64)>,
    on_set_target_completed: Callback<(i32, i64)>,
    on_remove_target: Callback<i32>,
    on_set_craft_choice: Callback<(i32, CraftMode, Option<i32>)>,
    on_set_on_hand: Callback<(i32, i64)>,
    on_add_step_crafted: Callback<(i32, i64)>,
    on_set_step_crafted: Callback<(i32, i64)>,
    on_set_options: Callback<(bool, bool)>,
    on_optimize: Callback<()>,
    on_go_shop: Callback<()>,
    on_open_add_recipe: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let grouping = RwSignal::new(StepGrouping::StepOrder);

    let target_ids = Memo::new(move |_| {
        project.with(|p| p.targets.iter().map(|t| t.recipe_id).collect::<Vec<_>>())
    });
    let step_ids =
        Memo::new(move |_| project.with(|p| p.steps.iter().map(|s| s.item_id).collect::<Vec<_>>()));
    let job_groups = Memo::new(move |_| {
        project.with(|p| {
            let mut by_job: BTreeMap<&'static str, Vec<i32>> = BTreeMap::new();
            for step in &p.steps {
                let key = if step.craft_type_abbr.is_empty() {
                    "—"
                } else {
                    step.craft_type_abbr
                };
                by_job.entry(key).or_default().push(step.item_id);
            }
            by_job.into_iter().collect::<Vec<_>>()
        })
    });

    view! {
        <div class="space-y-4" data-testid="list-craft-view">
            // Top Project Summary & Optimization Bar
            <section class="panel rounded-xl p-4 space-y-3" data-testid="craft-summary-bar">
                <div class="grid grid-cols-2 lg:grid-cols-4 gap-2.5">
                    <div class="craft-kpi space-y-0.5">
                        <p class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                            {t!(i18n, craft_summary_materials_cost)}
                        </p>
                        <p class="text-lg font-bold tabular-nums" data-testid="craft-materials-cost">
                            {move || project.with(|p| {
                                p.summary
                                    .materials_cost
                                    .map(|c| t_string!(i18n, lists_workspace_gil, price = format_gil(c)).to_string())
                                    .unwrap_or_else(|| "—".to_string())
                            })}
                        </p>
                        <Show when=move || project.with(|p| p.summary.unpriced_leaves > 0)>
                            <p class="text-xs text-[color:var(--color-text-muted)]">
                                {move || project.with(|p| t_string!(i18n, craft_summary_unpriced, count = p.summary.unpriced_leaves).to_string())}
                            </p>
                        </Show>
                    </div>

                    <div class="craft-kpi space-y-0.5">
                        <p class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                            {t!(i18n, craft_summary_vs_buy_finished)}
                        </p>
                        <p class="text-lg font-bold tabular-nums" data-testid="craft-vs-finished">
                            {move || project.with(|p| {
                                match p.summary.savings {
                                    Some(diff) if diff >= 0 => t_string!(i18n, craft_summary_save_gil, gil = format_gil(diff)).to_string(),
                                    Some(diff) => t_string!(i18n, craft_summary_more_gil, gil = format_gil(-diff)).to_string(),
                                    None => p.summary.buy_finished_cost
                                        .map(|c| t_string!(i18n, lists_workspace_gil, price = format_gil(c)).to_string())
                                        .unwrap_or_else(|| "—".to_string()),
                                }
                            })}
                        </p>
                        <Show when=move || project.with(|p| p.summary.savings.is_some())>
                            <p
                                class="text-xs tabular-nums"
                                class:text-positive=move || project.with(|p| p.summary.savings.is_some_and(|s| s >= 0))
                                class:text-negative=move || project.with(|p| p.summary.savings.is_some_and(|s| s < 0))
                            >
                                {move || project.with(|p| {
                                    p.summary.buy_finished_cost
                                        .map(|c| t_string!(i18n, craft_target_buy_finished, gil = format_gil(c)).to_string())
                                        .unwrap_or_default()
                                })}
                            </p>
                        </Show>
                    </div>

                    <div class="craft-kpi space-y-1.5">
                        <p class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                            {t!(i18n, craft_summary_steps_progress)}
                        </p>
                        <p class="text-sm font-semibold tabular-nums" data-testid="craft-progress-fraction">
                            {move || project.with(|p| {
                                t_string!(
                                    i18n,
                                    craft_summary_steps_fraction,
                                    done = p.summary.steps_done,
                                    total = p.summary.steps_total,
                                    targets_done = p.summary.targets_done,
                                    targets_total = p.summary.targets_total
                                )
                                .to_string()
                            })}
                        </p>
                        <div class="craft-progress-track">
                            <div
                                class="craft-progress-fill"
                                data-complete=move || project.with(|p| {
                                    let total = p.summary.steps_total + p.summary.targets_total;
                                    let done = p.summary.steps_done + p.summary.targets_done;
                                    (total > 0 && done >= total).to_string()
                                })
                                style:width=move || project.with(|p| {
                                    let total = p.summary.steps_total + p.summary.targets_total;
                                    let done = p.summary.steps_done + p.summary.targets_done;
                                    let pct = (done * 100).checked_div(total).unwrap_or(0);
                                    format!("{pct}%")
                                })
                            ></div>
                        </div>
                    </div>

                    <div class="craft-kpi space-y-1.5">
                        <p class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                            {t!(i18n, craft_summary_mats_acquired)}
                        </p>
                        <p class="text-lg font-bold tabular-nums" data-testid="craft-leaves-acquired">
                            {move || project.with(|p| format!("{} / {}", p.summary.leaves_acquired, p.summary.leaves_needed))}
                        </p>
                        <div class="craft-progress-track">
                            <div
                                class="craft-progress-fill"
                                data-complete=move || project.with(|p| (p.summary.leaves_needed > 0 && p.summary.leaves_acquired >= p.summary.leaves_needed).to_string())
                                style:width=move || project.with(|p| {
                                    if p.summary.leaves_needed <= 0 {
                                        "100%".to_string()
                                    } else {
                                        let pct = (p.summary.leaves_acquired * 100 / p.summary.leaves_needed).clamp(0, 100);
                                        format!("{pct}%")
                                    }
                                })
                            ></div>
                        </div>
                    </div>
                </div>

                {move || project.with(|p| p.error.clone()).map(|err| view! {
                    <p role="alert" class="text-sm text-negative" data-testid="craft-project-error">{err}</p>
                })}

                <div class="flex flex-wrap items-center justify-between gap-2 pt-1">
                    <div class="flex flex-wrap items-center gap-2">
                        <button
                            type="button"
                            class="btn-primary min-h-10 text-sm"
                            data-testid="craft-optimize-btn"
                            disabled=move || !can_write.get() || project.with(|p| p.steps.is_empty())
                            on:click=move |_| on_optimize.run(())
                        >
                            <Icon icon=i::FaHammerSolid aria_hidden=true />
                            <span>{t!(i18n, craft_summary_optimize)}</span>
                        </button>
                        <button
                            type="button"
                            class="btn-secondary min-h-10 text-sm"
                            data-testid="craft-go-shop"
                            on:click=move |_| on_go_shop.run(())
                        >
                            <span>{t!(i18n, craft_summary_go_shop)}</span>
                            <Icon icon=i::AiArrowRightOutlined aria_hidden=true />
                        </button>
                        <Show when=move || can_write.get()>
                            <button
                                type="button"
                                class="btn-ghost min-h-10 text-sm"
                                data-testid="craft-add-recipe"
                                on:click=move |_| on_open_add_recipe.run(())
                            >
                                {t!(i18n, craft_summary_add_recipe)}
                            </button>
                        </Show>
                    </div>

                    <div class="flex flex-wrap items-center gap-3 text-sm">
                        <label class="inline-flex items-center gap-1.5 cursor-pointer">
                            <input
                                type="checkbox"
                                class="h-4 w-4"
                                data-testid="craft-exclude-crystals"
                                disabled=move || !can_write.get()
                                prop:checked=move || project.with(|p| p.options.exclude_crystals)
                                on:change=move |ev| {
                                    let inc = project.with_untracked(|p| p.options.include_vendors);
                                    on_set_options.run((event_target_checked(&ev), inc));
                                }
                            />
                            <span>{t!(i18n, craft_summary_exclude_crystals)}</span>
                        </label>
                        <label class="inline-flex items-center gap-1.5 cursor-pointer">
                            <input
                                type="checkbox"
                                class="h-4 w-4"
                                data-testid="craft-include-vendors"
                                disabled=move || !can_write.get()
                                prop:checked=move || project.with(|p| p.options.include_vendors)
                                on:change=move |ev| {
                                    let exc = project.with_untracked(|p| p.options.exclude_crystals);
                                    on_set_options.run((exc, event_target_checked(&ev)));
                                }
                            />
                            <span>{t!(i18n, craft_summary_include_vendors)}</span>
                        </label>
                        <div class="segmented-control" role="group" data-testid="craft-group-toggle">
                            <button
                                type="button"
                                class="segmented-option !min-h-9 !py-1 !px-2.5 text-xs"
                                data-testid="craft-group-step-order"
                                aria-pressed=move || (grouping.get() == StepGrouping::StepOrder).to_string()
                                on:click=move |_| grouping.set(StepGrouping::StepOrder)
                            >
                                {t!(i18n, craft_group_step_order)}
                            </button>
                            <button
                                type="button"
                                class="segmented-option !min-h-9 !py-1 !px-2.5 text-xs"
                                data-testid="craft-group-by-job"
                                aria-pressed=move || (grouping.get() == StepGrouping::ByJob).to_string()
                                on:click=move |_| grouping.set(StepGrouping::ByJob)
                            >
                                {t!(i18n, craft_group_by_job)}
                            </button>
                        </div>
                    </div>
                </div>
            </section>

            // Section 1: Finished Targets
            <section class="space-y-2.5" data-testid="craft-targets-section">
                <h2 class="text-base font-semibold text-[color:var(--brand-fg)]">
                    {t!(i18n, craft_targets_section_title)}
                </h2>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <For
                        each=move || target_ids.get()
                        key=|recipe_id| *recipe_id
                        children=move |recipe_id| {
                            let target = Memo::new(move |_| {
                                project.with(|p| {
                                    p.targets
                                        .iter()
                                        .find(|t| t.recipe_id == recipe_id)
                                        .cloned()
                                })
                            });
                            view! {
                                <TargetCard
                                    target
                                    can_write
                                    on_set_target_count
                                    on_add_target_completed
                                    on_set_target_completed
                                    on_remove_target
                                />
                            }
                        }
                    />
                </div>
            </section>

            // Section 2: Intermediate Sub-Crafts
            <section class="space-y-2.5" data-testid="craft-steps-section">
                <h2 class="text-base font-semibold text-[color:var(--brand-fg)]">
                    {t!(i18n, craft_steps_section_title)}
                </h2>
                <Show
                    when=move || !step_ids.with(|s| s.is_empty())
                    fallback=move || view! {
                        <p class="panel rounded-xl p-4 text-sm text-[color:var(--color-text-muted)]" data-testid="craft-steps-empty">
                            {t!(i18n, craft_steps_empty)}
                        </p>
                    }
                >
                    <Show
                        when=move || grouping.get() == StepGrouping::StepOrder
                        fallback=move || view! {
                            <div class="space-y-4" data-testid="craft-steps-by-job">
                                <For
                                    each=move || job_groups.get()
                                    key=|(job, _)| *job
                                    children=move |(job, ids)| {
                                        view! {
                                            <div class="space-y-2" data-job-group=job>
                                                <h3 class="text-sm font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                    {job}
                                                </h3>
                                                <div class="space-y-2.5">
                                                    {ids.into_iter().map(|item_id| {
                                                        let step = Memo::new(move |_| {
                                                            project.with(|p| p.steps.iter().find(|s| s.item_id == item_id).cloned())
                                                        });
                                                        view! {
                                                            <StepCard
                                                                step
                                                                can_write
                                                                on_set_craft_choice
                                                                on_set_on_hand
                                                                on_add_step_crafted
                                                                on_set_step_crafted
                                                            />
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            </div>
                                        }
                                    }
                                />
                            </div>
                        }
                    >
                        <div class="space-y-2.5" data-testid="craft-steps-list">
                            <For
                                each=move || step_ids.get()
                                key=|item_id| *item_id
                                children=move |item_id| {
                                    let step = Memo::new(move |_| {
                                        project.with(|p| {
                                            p.steps.iter().find(|s| s.item_id == item_id).cloned()
                                        })
                                    });
                                    view! {
                                        <StepCard
                                            step
                                            can_write
                                            on_set_craft_choice
                                            on_set_on_hand
                                            on_add_step_crafted
                                            on_set_step_crafted
                                        />
                                    }
                                }
                            />
                        </div>
                    </Show>
                </Show>
            </section>

            // Section 3: Consolidated Shop Preview Footer
            <section
                class="panel rounded-xl p-4 flex flex-wrap items-center justify-between gap-3"
                data-testid="craft-shop-footer"
            >
                <div class="space-y-0.5 min-w-0">
                    <h2 class="text-sm font-semibold">{t!(i18n, craft_shop_footer_title)}</h2>
                    <p class="text-xs text-[color:var(--color-text-muted)] tabular-nums">
                        {move || project.with(|p| {
                            t_string!(
                                i18n,
                                craft_shop_footer_summary,
                                acquired = p.summary.leaves_acquired,
                                needed = p.summary.leaves_needed
                            )
                            .to_string()
                        })}
                    </p>
                </div>
                <button
                    type="button"
                    class="btn-primary min-h-10 text-sm shrink-0"
                    data-testid="craft-footer-shop"
                    on:click=move |_| on_go_shop.run(())
                >
                    <span>{t!(i18n, craft_shop_footer_open)}</span>
                    <Icon icon=i::AiArrowRightOutlined aria_hidden=true />
                </button>
            </section>
        </div>
    }
}

#[component]
fn TargetCard(
    target: Memo<Option<CraftTargetView>>,
    can_write: Signal<bool>,
    on_set_target_count: Callback<(i32, i64)>,
    on_add_target_completed: Callback<(i32, i64)>,
    on_set_target_completed: Callback<(i32, i64)>,
    on_remove_target: Callback<i32>,
) -> impl IntoView {
    let i18n = use_i18n();
    move || {
        let t = target.get()?;
        let recipe_id = t.recipe_id;
        let (pill_class, pill_text) = readiness_badge(i18n, t.readiness);
        let plus_step = t.yield_amount.min((t.count - t.completed).max(1));
        let is_done = t.completed >= t.count;
        Some(view! {
            <article
                class="craft-card space-y-2.5"
                data-testid="craft-target-card"
                data-target-recipe-id=recipe_id
            >
                <div class="flex items-start gap-2.5">
                    <div class="shrink-0">
                        <ItemIcon item_id=t.item_id icon_size=IconSize::Small />
                    </div>
                    <div class="min-w-0 flex-1 space-y-1">
                        <div class="flex flex-wrap items-center gap-1.5">
                            <a
                                class="font-semibold hover:underline truncate"
                                href=format!("/recipe/{}", recipe_id)
                            >
                                {t.item_name.clone()}
                            </a>
                            <Show when=move || !t.craft_type_abbr.is_empty()>
                                <span class="craft-pill">{format!("{} Lv.{}", t.craft_type_abbr, t.recipe_level)}</span>
                            </Show>
                        </div>
                        <p class="text-xs text-[color:var(--color-text-muted)] tabular-nums">
                            {t_string!(i18n, craft_target_yield_note, crafts = t.crafts, surplus = t.surplus).to_string()}
                            {t.buy_finished_cost.map(|c| format!(" · {}", t_string!(i18n, craft_target_buy_finished, gil = format_gil(c)))).unwrap_or_default()}
                        </p>
                    </div>
                    <span class=pill_class>{pill_text}</span>
                </div>

                <div class="flex flex-wrap items-center justify-between gap-2 pt-1 border-t border-[color:var(--color-outline)]">
                    <label class="inline-flex items-center gap-1.5 text-xs text-[color:var(--color-text-muted)]">
                        <span>{t!(i18n, craft_targets_count_label)}</span>
                        <input
                            type="number"
                            min="1"
                            max="9999"
                            class="input w-20 text-sm py-1"
                            data-testid="craft-target-count"
                            disabled=move || !can_write.get()
                            prop:value=t.count
                            on:change=move |ev| {
                                if let Ok(v) = event_target_value(&ev).parse::<i64>() {
                                    on_set_target_count.run((recipe_id, v.clamp(1, 9999)));
                                }
                            }
                        />
                    </label>
                    <div class="flex flex-wrap items-center gap-1.5">
                        <span class="text-xs text-[color:var(--color-text-muted)] tabular-nums mr-1">
                            {t_string!(i18n, craft_targets_completed_of, completed = t.completed, total = t.count).to_string()}
                        </span>
                        <Show when=move || can_write.get()>
                            <button
                                type="button"
                                class="btn-secondary !py-1 !px-2 text-xs min-h-8"
                                data-testid="craft-target-plus"
                                disabled=is_done
                                on:click=move |_| on_add_target_completed.run((recipe_id, plus_step))
                            >
                                {t!(i18n, craft_plus_one)}
                            </button>
                            <button
                                type="button"
                                class="btn-secondary !py-1 !px-2 text-xs min-h-8"
                                data-testid="craft-target-done"
                                disabled=is_done
                                on:click=move |_| on_set_target_completed.run((recipe_id, t.count))
                            >
                                {t!(i18n, craft_mark_done)}
                            </button>
                            <button
                                type="button"
                                class="btn-ghost !p-1.5 min-h-8 min-w-8 text-[color:var(--color-text-muted)] hover:text-negative"
                                data-testid="craft-target-remove"
                                aria-label=t_string!(i18n, craft_remove_target)
                                title=t_string!(i18n, craft_remove_target)
                                on:click=move |_| on_remove_target.run(recipe_id)
                            >
                                <Icon icon=i::BiTrashSolid aria_hidden=true />
                            </button>
                        </Show>
                    </div>
                </div>
            </article>
        })
    }
}

#[component]
fn StepCard(
    step: Memo<Option<CraftStepView>>,
    can_write: Signal<bool>,
    on_set_craft_choice: Callback<(i32, CraftMode, Option<i32>)>,
    on_set_on_hand: Callback<(i32, i64)>,
    on_add_step_crafted: Callback<(i32, i64)>,
    on_set_step_crafted: Callback<(i32, i64)>,
) -> impl IntoView {
    let i18n = use_i18n();
    move || {
        let s = step.get()?;
        let item_id = s.item_id;
        let recipe_id = s.recipe_id;
        let is_craft = s.mode == CraftMode::Craft;
        let (pill_class, pill_text) = readiness_badge(i18n, s.readiness);
        let consumers_line = (!s.consumers.is_empty()).then(|| {
            let joined = s
                .consumers
                .iter()
                .map(|c| format!("{} ×{}", c.name, c.units_used))
                .collect::<Vec<_>>()
                .join(", ");
            t_string!(i18n, craft_step_used_by, consumers = joined).to_string()
        });
        let craft_cheaper = matches!((s.craft_cost, s.buy_cost), (Some(c), Some(b)) if c <= b);
        let buy_cheaper = matches!((s.craft_cost, s.buy_cost), (Some(c), Some(b)) if b < c);
        let ing_count = s.ingredients.len();
        let show_ingredients = is_craft && !s.ingredients.is_empty();
        let ingredient_items = StoredValue::new(s.ingredients.clone());
        let step_done = s.crafted >= s.crafts;

        Some(view! {
            <article
                class="craft-step-card space-y-2.5"
                data-testid="craft-step-card"
                data-step-item-id=item_id
                data-craft-mode=s.mode.as_str()
                data-ready=step_readiness_attr(s.readiness)
            >
                <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div class="flex items-start gap-2.5 min-w-0 flex-1">
                        <div class="shrink-0">
                            <ItemIcon item_id=s.item_id icon_size=IconSize::Small />
                        </div>
                        <div class="min-w-0 flex-1 space-y-0.5">
                            <div class="flex flex-wrap items-center gap-1.5">
                                <span class="craft-pill">{t_string!(i18n, craft_step_label, num = s.step_number).to_string()}</span>
                                <a
                                    class="font-semibold hover:underline break-words sm:truncate"
                                    href=format!("/recipe/{}", recipe_id)
                                >
                                    {s.item_name.clone()}
                                </a>
                                <Show when=move || !s.craft_type_abbr.is_empty()>
                                    <span class="craft-pill">{format!("{} Lv.{}", s.craft_type_abbr, s.recipe_level)}</span>
                                </Show>
                            </div>
                            <p class="text-xs text-[color:var(--color-text-muted)] tabular-nums">
                                {t_string!(i18n, craft_step_summary, needed = s.needed, crafts = s.crafts).to_string()}
                                {if s.surplus > 0 { format!(" · {}", t_string!(i18n, craft_step_surplus, surplus = s.surplus)) } else { String::new() }}
                            </p>
                            {consumers_line.map(|line| view! {
                                <p class="text-xs text-[color:var(--color-text-muted)] break-words sm:truncate">
                                    {line}
                                </p>
                            })}
                        </div>
                    </div>

                    <div class="flex flex-wrap items-center gap-2 shrink-0">
                        <span class=pill_class>{pill_text}</span>
                        <div class="segmented-control" role="group" data-testid="craft-choice-toggle">
                            <button
                                type="button"
                                class="segmented-option !min-h-8 !py-0.5 !px-2.5 text-xs"
                                data-testid="craft-choice-craft"
                                disabled=move || !can_write.get()
                                aria-pressed=is_craft.to_string()
                                on:click=move |_| on_set_craft_choice.run((item_id, CraftMode::Craft, Some(recipe_id)))
                            >
                                {t!(i18n, craft_choice_craft)}
                            </button>
                            <button
                                type="button"
                                class="segmented-option !min-h-8 !py-0.5 !px-2.5 text-xs"
                                data-testid="craft-choice-buy"
                                disabled=move || !can_write.get()
                                aria-pressed=(!is_craft).to_string()
                                on:click=move |_| on_set_craft_choice.run((item_id, CraftMode::Buy, Some(recipe_id)))
                            >
                                {t!(i18n, craft_choice_buy)}
                            </button>
                        </div>
                    </div>
                </div>

                // Craft vs Buy cost comparison + Stock & Craft progress controls
                <div class="flex flex-wrap items-center justify-between gap-2 pt-2 border-t border-[color:var(--color-outline)] text-xs">
                    <div class="flex flex-wrap items-center gap-3 tabular-nums">
                        <span class:text-positive=craft_cheaper class:font-semibold=craft_cheaper>
                            {s.craft_cost.map(|c| t_string!(i18n, craft_cost_craft_label, gil = format_gil(c)).to_string()).unwrap_or_else(|| t_string!(i18n, craft_cost_missing).to_string())}
                        </span>
                        <span class:text-positive=buy_cheaper class:font-semibold=buy_cheaper>
                            {s.buy_cost.map(|b| t_string!(i18n, craft_cost_buy_label, gil = format_gil(b)).to_string()).unwrap_or_else(|| t_string!(i18n, craft_cost_missing).to_string())}
                        </span>
                    </div>

                    <div class="flex flex-wrap items-center gap-2">
                        <label class="inline-flex items-center gap-1.5 text-[color:var(--color-text-muted)]">
                            <span>{t!(i18n, craft_step_on_hand)}</span>
                            <input
                                type="number"
                                min="0"
                                max="1000000"
                                class="input w-20 text-xs py-1"
                                data-testid="craft-on-hand-input"
                                disabled=move || !can_write.get()
                                prop:value=s.on_hand
                                on:change=move |ev| {
                                    if let Ok(v) = event_target_value(&ev).parse::<i64>() {
                                        on_set_on_hand.run((item_id, v.clamp(0, 1_000_000)));
                                    }
                                }
                            />
                        </label>
                        <Show
                            when=move || is_craft
                            fallback=move || view! {
                                <span class="text-[color:var(--color-text-muted)] italic">
                                    {t!(i18n, craft_step_buying_note)}
                                </span>
                            }
                        >
                            <span class="tabular-nums text-[color:var(--color-text-muted)]" data-testid="craft-step-crafted-progress">
                                {t_string!(i18n, craft_step_crafted_progress, done = s.crafted, total = s.crafts).to_string()}
                            </span>
                            <Show when=move || can_write.get()>
                                <button
                                    type="button"
                                    class="btn-secondary !py-1 !px-2 text-xs min-h-8"
                                    data-testid="craft-step-plus"
                                    disabled=step_done
                                    on:click=move |_| on_add_step_crafted.run((item_id, 1))
                                >
                                    {t!(i18n, craft_plus_one)}
                                </button>
                                <button
                                    type="button"
                                    class="btn-secondary !py-1 !px-2 text-xs min-h-8"
                                    data-testid="craft-step-done"
                                    disabled=step_done
                                    on:click=move |_| on_set_step_crafted.run((item_id, s.crafts))
                                >
                                    {t!(i18n, craft_mark_done)}
                                </button>
                            </Show>
                        </Show>
                    </div>
                </div>

                <Show when=move || show_ingredients>
                    <details class="craft-ingredients-details pt-1">
                        <summary>{t_string!(i18n, craft_step_ingredients_toggle, count = ing_count).to_string()}</summary>
                        <ul class="mt-2 grid grid-cols-1 sm:grid-cols-2 gap-1.5 text-xs">
                            {ingredient_items.with_value(|items| items.iter().filter(|ing| ing.needed_total > 0).map(|ing| {
                                let covered = ing.available >= ing.needed_total;
                                let ing_name = ing.name.clone();
                                let ing_id = ing.item_id;
                                let row_text = t_string!(
                                    i18n,
                                    craft_step_ingredient_row,
                                    available = ing.available,
                                    needed = ing.needed_total,
                                    name = ing_name
                                )
                                .to_string();
                                view! {
                                    <li class="flex items-center gap-2 rounded-md border border-[color:var(--color-outline)] px-2 py-1 bg-[color:var(--color-background)]">
                                        <div class="shrink-0">
                                            <ItemIcon item_id=ing_id icon_size=IconSize::Small />
                                        </div>
                                        <span
                                            class="truncate tabular-nums"
                                            class:text-positive=covered
                                        >
                                            {row_text}
                                        </span>
                                    </li>
                                }
                            }).collect_view())}
                        </ul>
                    </details>
                </Show>
            </article>
        })
    }
}
