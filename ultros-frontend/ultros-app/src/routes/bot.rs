use leptos::prelude::*;
use leptos_meta::{Meta, Title};

#[component]
pub fn BotGuide() -> impl IntoView {
    view! {
        <Title text="Ultros Discord Bot" />
        <Meta
            name="description"
            content="Command reference and setup guide for the Ultros Discord bot — FFXIV market data in your server."
        />
        <div class="container mx-auto max-w-4xl px-4 py-8 flex flex-col gap-12">
            <header class="flex flex-col gap-3">
                <h1 class="text-4xl font-bold text-brand-200">"Ultros Discord Bot"</h1>
                <p class="text-lg text-[color:var(--color-text-muted)]">
                    "Look up market prices, manage retainers, and get undercut alerts — all from Discord."
                </p>
            </header>

            <section id="invite" class="flex flex-col gap-4">
                <h2 class="text-2xl font-semibold text-brand-300">"1. Invite the bot"</h2>
                <p>
                    "Adds the Ultros bot to your server with the "
                    <code class="rounded bg-black/40 px-1.5 py-0.5">"Use Application Commands"</code>
                    " permission. You can configure per-channel access from Discord's server settings."
                </p>
                <a
                    href="/invitebot"
                    class="self-start rounded-md bg-brand-500 px-5 py-2.5 font-semibold text-white shadow hover:bg-brand-400 transition-colors"
                >
                    "Add to your server"
                </a>
            </section>

            <section id="getting-started" class="flex flex-col gap-3">
                <h2 class="text-2xl font-semibold text-brand-300">"2. Getting started"</h2>
                <ol class="list-decimal list-inside flex flex-col gap-2">
                    <li>"Verify your FFXIV character on this site (Settings → Characters → Lodestone challenge)."</li>
                    <li>"In Discord, run " <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add"</code> " — the autocomplete only shows retainers belonging to your verified characters."</li>
                    <li>"In any channel where the bot is installed, run " <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_undercut_alert margin_percent:0"</code> " — you'll get a ping the moment a competitor undercuts you."</li>
                </ol>
            </section>

            <section id="commands" class="flex flex-col gap-8">
                <h2 class="text-2xl font-semibold text-brand-300">"3. Command reference"</h2>

                <CommandGroup
                    title="/ffxiv prices".to_string()
                    description="Real-time market lookups.".to_string()
                    commands=vec![
                        ("/ffxiv prices current item:<name> world:<world>", "Top 10 cheapest current listings."),
                        ("/ffxiv prices history item:<name> world:<world>", "Historical price chart (PNG)."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv retainer".to_string()
                    description="Manage your retainers. Requires a verified character.".to_string()
                    commands=vec![
                        ("/ffxiv retainer list", "Show your retainers and their listing counts."),
                        ("/ffxiv retainer add retainer_id:<name>", "Claim a retainer (autocomplete only shows your own)."),
                        ("/ffxiv retainer remove owned_retainer_id:<name>", "Release a retainer claim."),
                        ("/ffxiv retainer check_listings", "All your active listings, tabled."),
                        ("/ffxiv retainer check_undercuts", "Only your listings that have been undercut."),
                        ("/ffxiv retainer add_undercut_alert margin_percent:<0-200>", "Notify this channel on undercut."),
                        ("/ffxiv retainer remove_undercut_alert", "Stop notifications in this channel."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv list".to_string()
                    description="Shopping lists scoped to a region/datacenter/world.".to_string()
                    commands=vec![
                        ("/ffxiv list show_lists", "Show your list names."),
                        ("/ffxiv list create list_name:<name> region_datacenter_or_world:<scope>", "Create a list."),
                        ("/ffxiv list remove list_name:<name>", "Delete a list."),
                        ("/ffxiv list add_item list_name:<name> item_name:<item> [quantity] [hq]", "Add to a list."),
                        ("/ffxiv list remove_item list_name:<name> item_name:<item>", "Remove from a list."),
                        ("/ffxiv list show_list list_name:<name>", "Show current lowest prices for the list."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv analyze".to_string()
                    description="Market analysis.".to_string()
                    commands=vec![
                        ("/ffxiv analyze profit world:<name> [minimum_profit=10000] [number_recently_sold=5] [threshold_days=7]", "Top 15 flips on a world."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv character".to_string()
                    description="Lodestone lookup. To verify ownership, use Settings on this site.".to_string()
                    commands=vec![
                        ("/ffxiv character register name:<First Last> [home_world]", "Search Lodestone."),
                    ]
                />
            </section>
        </div>
    }
}

#[component]
fn CommandGroup(
    title: String,
    description: String,
    commands: Vec<(&'static str, &'static str)>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-3 rounded-lg border border-brand-500/20 bg-black/20 p-5">
            <div class="flex flex-col gap-1">
                <h3 class="text-xl font-semibold text-brand-200">{title}</h3>
                <p class="text-sm text-[color:var(--color-text-muted)]">{description}</p>
            </div>
            <ul class="flex flex-col gap-2">
                {commands.into_iter().map(|(sig, desc)| view! {
                    <li class="flex flex-col gap-0.5">
                        <code class="text-sm rounded bg-black/40 px-2 py-1 self-start">{sig}</code>
                        <span class="text-sm text-[color:var(--color-text-muted)] pl-1">{desc}</span>
                    </li>
                }).collect_view()}
            </ul>
        </div>
    }
}
