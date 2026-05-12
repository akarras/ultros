use crate::components::{
    icon::Icon,
    meta::{MetaDescription, MetaTitle},
};
use icondata as i;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};

#[derive(Clone, Copy, PartialEq)]
pub struct HelpTopic {
    pub slug: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub summary: &'static str,
    pub purpose: &'static str,
    pub inputs: &'static [&'static str],
    pub assumptions: &'static [&'static str],
    pub results: &'static [&'static str],
    pub next_actions: &'static [&'static str],
}

pub const HELP_TOPICS: &[HelpTopic] = &[
    HelpTopic {
        slug: "getting-started",
        title: "Getting started",
        category: "Basics",
        summary: "Set a home world, choose price defaults, and learn how Ultros turns market data into recommendations.",
        purpose: "Use this when you are new to Ultros or when prices appear to be coming from the wrong world.",
        inputs: &["Home world", "Default price zone", "Language preference"],
        assumptions: &[
            "A home world lets tools preselect useful worlds and regions.",
            "Most tools can still run without an account.",
        ],
        results: &[
            "Personalized links, world-aware market pages, and better defaults across tools.",
        ],
        next_actions: &[
            "Open Settings to adjust your home world.",
            "Use global search to jump to an item or tool.",
        ],
    },
    HelpTopic {
        slug: "flip-finder",
        title: "Flip Finder",
        category: "Market analysis",
        summary: "Find items that may be profitable to buy on one world and sell on another.",
        purpose: "Use this when you want buy-low/sell-high opportunities with sales velocity and ROI filters.",
        inputs: &[
            "Selected sell world",
            "Recent sales",
            "Cheapest listings",
            "Cross-region setting",
            "Tax setting",
        ],
        assumptions: &[
            "Profit subtracts the buy price from the estimated sell price.",
            "Tax is included when enabled.",
            "Outlier filtering removes unusual sales from averages.",
        ],
        results: &[
            "Profit estimates the gil after buying and reselling.",
            "ROI shows profit relative to purchase cost.",
            "Average sale time and sales count indicate confidence.",
        ],
        next_actions: &[
            "Inspect the item page.",
            "Add candidates to a list.",
            "Tighten filters for sales speed or purchase budget.",
        ],
    },
    HelpTopic {
        slug: "vendor-resale",
        title: "Vendor Resale",
        category: "Market analysis",
        summary: "Find NPC vendor items that may resell for more on the market board.",
        purpose: "Use this for low-complexity opportunities where the buy price is fixed by an NPC vendor.",
        inputs: &[
            "Selected world",
            "Vendor price",
            "Current NQ market listing",
            "Recent sales",
        ],
        assumptions: &[
            "Vendor purchases are treated as NQ.",
            "HQ market listings are excluded.",
            "Specific vendor NPC names are not shown yet.",
        ],
        results: &[
            "Profit compares vendor cost to the market listing.",
            "ROI is usually high because vendor cost is often low.",
            "Sales pace matters more than raw ROI.",
        ],
        next_actions: &[
            "Favor items with recent sales.",
            "Check the item page before buying in bulk.",
        ],
    },
    HelpTopic {
        slug: "recipe-analyzer",
        title: "Recipe Analyzer",
        category: "Crafting",
        summary: "Compare crafted item sale prices against ingredient costs for your crafting levels.",
        purpose: "Use this to find recipes you can craft profitably with market-board ingredients.",
        inputs: &[
            "Crafter levels",
            "Ingredient listings",
            "Output item listings",
            "Recent sales",
        ],
        assumptions: &[
            "Ingredient costs use cheapest matching listings.",
            "Subcrafts are optional and recurse only a limited depth.",
            "Missing prices remove or heavily penalize a recipe.",
        ],
        results: &[
            "Profit is market price minus estimated craft cost.",
            "Velocity shows whether the output sells often enough to matter.",
            "Subcraft detail shows when crafting intermediate ingredients saves gil.",
        ],
        next_actions: &[
            "Configure crafter levels.",
            "Open item details.",
            "Add profitable recipes to a list.",
        ],
    },
    HelpTopic {
        slug: "leve-analyzer",
        title: "Leve Analyzer",
        category: "Crafting",
        summary: "Estimate levequest gil value after buying or crafting turn-in items.",
        purpose: "Use this when choosing leveling leves with useful market value.",
        inputs: &[
            "Selected world",
            "Turn-in item price",
            "Gil reward",
            "Expected item reward value",
            "Recent sales",
        ],
        assumptions: &[
            "Calculations use baseline NQ turn-ins.",
            "Some reward value is expected value, not a guaranteed payout.",
        ],
        results: &[
            "Revenue includes gil plus estimated reward item value.",
            "Profit subtracts the turn-in item cost.",
            "Level and job filters narrow the leveling path.",
        ],
        next_actions: &[
            "Filter by job.",
            "Check item supply before relying on expected rewards.",
        ],
    },
    HelpTopic {
        slug: "fc-crafting",
        title: "FC Crafting Analyzer",
        category: "Crafting",
        summary: "Estimate Free Company project profitability from required materials and output sales.",
        purpose: "Use this before committing workshop materials to airship or submersible projects.",
        inputs: &[
            "Project material list",
            "Material listings",
            "Output listing",
            "Recent output sales",
        ],
        assumptions: &[
            "Costs are derived from company craft material requirements.",
            "Sparse sales should be treated cautiously.",
        ],
        results: &[
            "Total cost sums market materials.",
            "ROI compares projected profit to material cost.",
            "Confidence depends on recent output sales.",
        ],
        next_actions: &[
            "Review material breakdowns.",
            "Prefer projects with both profit and sales activity.",
        ],
    },
    HelpTopic {
        slug: "scrip-sources",
        title: "Scrip Sources",
        category: "Currencies",
        summary: "Find collectables with the lowest gil cost per scrip.",
        purpose: "Use this to choose efficient crafted collectables for scrip farming.",
        inputs: &[
            "Scrip type",
            "Crafting job",
            "Ingredient listings",
            "Max collectability reward",
        ],
        assumptions: &[
            "Reward amount uses the high collectability value.",
            "Cost is based on market ingredients.",
            "Gathering sources are limited in this first pass.",
        ],
        results: &[
            "Cost per scrip is the main efficiency metric.",
            "Lower is better.",
            "Total cost helps avoid expensive turn-ins even when efficient.",
        ],
        next_actions: &[
            "Filter to the scrip color you need.",
            "Open the item before buying ingredients.",
        ],
    },
    HelpTopic {
        slug: "venture-analyzer",
        title: "Venture Analyzer",
        category: "Retainers",
        summary: "Rank normal retainer ventures by gross market value and sales activity.",
        purpose: "Use this to decide which ventures to send retainers on for sellable items.",
        inputs: &[
            "Retainer job category",
            "Task level",
            "Venture output quantity",
            "Market price",
            "Recent sales",
        ],
        assumptions: &[
            "Profit is gross revenue.",
            "Venture token cost and opportunity cost are not modeled yet.",
        ],
        results: &[
            "Profit multiplies output quantity by current market price.",
            "Daily sales helps separate practical choices from rare slow sellers.",
        ],
        next_actions: &[
            "Filter by retainer job.",
            "Check item history for slow-moving drops.",
        ],
    },
    HelpTopic {
        slug: "market-trends",
        title: "Market Trends",
        category: "Market analysis",
        summary: "Review high-velocity, rising-price, and falling-price items for a world.",
        purpose: "Use this to spot demand shifts before diving into individual item pages.",
        inputs: &["Selected world", "Recent price movement", "Sales per week"],
        assumptions: &["Trends are directional signals, not guaranteed future prices."],
        results: &[
            "High velocity means frequent sales.",
            "Rising prices may indicate demand or shortage.",
            "Falling prices may indicate oversupply.",
        ],
        next_actions: &[
            "Open the item page.",
            "Add watched items to a list.",
            "Compare with sale history.",
        ],
    },
    HelpTopic {
        slug: "lists-alerts-retainers",
        title: "Lists, alerts, and retainers",
        category: "Workflow",
        summary: "Turn market research into tracked shopping lists, alerts, and retainer checks.",
        purpose: "Use this when moving from analysis to repeated market-board work.",
        inputs: &[
            "Lists",
            "Price thresholds",
            "Discord delivery",
            "Claimed retainers",
        ],
        assumptions: &[
            "Alerts require login.",
            "Retainer undercut checks depend on claimed characters and market data freshness.",
        ],
        results: &[
            "Lists organize items for buying.",
            "Alerts notify when thresholds match.",
            "Retainers show listings and undercut checks.",
        ],
        next_actions: &[
            "Create a list.",
            "Add item alerts from list rows.",
            "Claim characters in Settings.",
        ],
    },
];

pub fn help_topic(slug: &str) -> Option<HelpTopic> {
    HELP_TOPICS.iter().copied().find(|topic| topic.slug == slug)
}

#[component]
fn TopicSection(title: &'static str, items: &'static [&'static str]) -> impl IntoView {
    view! {
        <section class="panel p-5 rounded-xl">
            <h2 class="text-lg font-bold text-[color:var(--brand-fg)] mb-3">{title}</h2>
            <ul class="space-y-2 text-sm text-[color:var(--color-text)]">
                {items.iter().map(|item| view! {
                    <li class="flex gap-2">
                        <Icon icon=i::BsCheck2Circle width="1em" height="1em" attr:class="mt-0.5 shrink-0 text-brand-300" />
                        <span>{*item}</span>
                    </li>
                }).collect_view()}
            </ul>
        </section>
    }
}

#[component]
pub fn HelpIndex() -> impl IntoView {
    view! {
        <MetaTitle title="Help - Ultros" />
        <MetaDescription text="Task-focused help for Ultros market, crafting, retainer, and currency tools." />
        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-7xl flex flex-col gap-6">
                <section class="panel p-6 sm:p-8 rounded-2xl">
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-3">"Help"</h1>
                    <p class="text-lg text-[color:var(--color-text)] max-w-3xl">
                        "Short, task-focused guides for understanding what each Ultros tool optimizes for, what data it uses, and what to do next."
                    </p>
                </section>
                <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
                    {HELP_TOPICS.iter().map(|topic| view! {
                        <A href=format!("/help/{}", topic.slug) attr:class="panel p-5 rounded-xl hover:border-brand-300 transition-colors flex flex-col gap-2">
                            <span class="text-xs uppercase tracking-wide text-brand-300 font-bold">{topic.category}</span>
                            <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{topic.title}</h2>
                            <p class="text-sm text-[color:var(--color-text-muted)]">{topic.summary}</p>
                        </A>
                    }).collect_view()}
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn HelpArticle() -> impl IntoView {
    let params = use_params_map();
    let topic = Memo::new(move |_| {
        params
            .with(|params| params.get("topic").clone())
            .and_then(|slug| help_topic(&slug))
    });

    view! {
        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-5xl flex flex-col gap-6">
                {move || match topic() {
                    Some(topic) => view! {
                        <MetaTitle title=format!("{} - Ultros Help", topic.title) />
                        <MetaDescription text=topic.summary />
                        <A href="/help" attr:class="text-sm text-brand-300 hover:text-[color:var(--brand-fg)] inline-flex items-center gap-2">
                            <Icon icon=i::FaArrowLeftSolid width="0.85em" height="0.85em" />
                            "All help topics"
                        </A>
                        <section class="panel p-6 sm:p-8 rounded-2xl">
                            <span class="text-xs uppercase tracking-wide text-brand-300 font-bold">{topic.category}</span>
                            <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mt-2 mb-3">{topic.title}</h1>
                            <p class="text-lg text-[color:var(--color-text)]">{topic.summary}</p>
                            <p class="mt-4 text-sm text-[color:var(--color-text-muted)]">{topic.purpose}</p>
                        </section>
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                            <TopicSection title="Inputs" items=topic.inputs />
                            <TopicSection title="Assumptions" items=topic.assumptions />
                            <TopicSection title="How to read results" items=topic.results />
                            <TopicSection title="Next actions" items=topic.next_actions />
                        </div>
                    }.into_any(),
                    None => view! {
                        <MetaTitle title="Help topic not found - Ultros" />
                        <section class="panel p-6 rounded-2xl text-center">
                            <h1 class="text-2xl font-bold text-[color:var(--brand-fg)]">"Help topic not found"</h1>
                            <p class="mt-2 text-[color:var(--color-text-muted)]">"That guide may have moved."</p>
                            <A href="/help" attr:class="btn-primary mt-4">"Browse help"</A>
                        </section>
                    }.into_any(),
                }}
            </div>
        </div>
    }
}
