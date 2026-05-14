use crate::components::meta::{MetaDescription, MetaTitle};
use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn BotGuide() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <MetaTitle title=t_string!(i18n, bot_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, bot_meta_desc).to_string() />
        <div class="container mx-auto max-w-4xl px-4 py-8 flex flex-col gap-12">
            <header class="flex flex-col gap-3">
                <h1 class="text-4xl font-bold text-brand-200">{t!(i18n, bot_heading)}</h1>
                <p class="text-lg text-[color:var(--color-text-muted)]">
                    {t!(i18n, bot_tagline)}
                </p>
            </header>

            <section id="invite" class="flex flex-col gap-4">
                <h2 class="text-2xl font-semibold text-brand-300">{t!(i18n, bot_section_invite_heading)}</h2>
                <p>
                    {t!(i18n, bot_invite_prefix)}
                    <code class="rounded bg-black/40 px-1.5 py-0.5">{t!(i18n, bot_permission_use_app_commands)}</code>
                    {t!(i18n, bot_invite_suffix)}
                </p>
                <a
                    href="/invitebot"
                    class="self-start rounded-md bg-brand-500 px-5 py-2.5 font-semibold text-white shadow hover:bg-brand-400 transition-colors"
                >
                    {t!(i18n, bot_invite_button)}
                </a>
            </section>

            <section id="getting-started" class="flex flex-col gap-3">
                <h2 class="text-2xl font-semibold text-brand-300">{t!(i18n, bot_section_getting_started_heading)}</h2>
                <ol class="list-decimal list-inside flex flex-col gap-2">
                    <li>{t!(i18n, bot_setup_step_verify)}</li>
                    <li>{t!(i18n, bot_setup_step_discord_run_prefix)} <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add"</code> {t!(i18n, bot_setup_step_discord_run_suffix)}</li>
                    <li>{t!(i18n, bot_setup_step_run_in_channel_prefix)} <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_undercut_alert margin_percent:0"</code> {t!(i18n, bot_setup_step_run_in_channel_suffix)}</li>
                </ol>
            </section>

            <section id="commands" class="flex flex-col gap-8">
                <h2 class="text-2xl font-semibold text-brand-300">{t!(i18n, bot_section_commands_heading)}</h2>

                <CommandGroup
                    title="/ffxiv prices"
                    description=t_string!(i18n, bot_cmd_prices_description).to_string()
                    commands=vec![
                        ("/ffxiv prices current item:<name> world:<world>", t_string!(i18n, bot_cmd_prices_current_desc).to_string()),
                        ("/ffxiv prices history item:<name> world:<world>", t_string!(i18n, bot_cmd_prices_history_desc).to_string()),
                    ]
                />

                <CommandGroup
                    title="/ffxiv retainer"
                    description=t_string!(i18n, bot_cmd_retainer_description).to_string()
                    commands=vec![
                        ("/ffxiv retainer list", t_string!(i18n, bot_cmd_retainer_list_desc).to_string()),
                        ("/ffxiv retainer add retainer_id:<name>", t_string!(i18n, bot_cmd_retainer_add_desc).to_string()),
                        ("/ffxiv retainer remove owned_retainer_id:<name>", t_string!(i18n, bot_cmd_retainer_remove_desc).to_string()),
                        ("/ffxiv retainer check_listings", t_string!(i18n, bot_cmd_retainer_check_listings_desc).to_string()),
                        ("/ffxiv retainer check_undercuts", t_string!(i18n, bot_cmd_retainer_check_undercuts_desc).to_string()),
                        ("/ffxiv retainer add_undercut_alert margin_percent:<0-200>", t_string!(i18n, bot_cmd_retainer_add_undercut_alert_desc).to_string()),
                        ("/ffxiv retainer remove_undercut_alert", t_string!(i18n, bot_cmd_retainer_remove_undercut_alert_desc).to_string()),
                    ]
                />

                <CommandGroup
                    title="/ffxiv list"
                    description=t_string!(i18n, bot_cmd_list_description).to_string()
                    commands=vec![
                        ("/ffxiv list show_lists", t_string!(i18n, bot_cmd_list_show_lists_desc).to_string()),
                        ("/ffxiv list create list_name:<name> region_datacenter_or_world:<scope>", t_string!(i18n, bot_cmd_list_create_desc).to_string()),
                        ("/ffxiv list remove list_name:<name>", t_string!(i18n, bot_cmd_list_remove_desc).to_string()),
                        ("/ffxiv list add_item list_name:<name> item_name:<item> [quantity] [hq]", t_string!(i18n, bot_cmd_list_add_item_desc).to_string()),
                        ("/ffxiv list remove_item list_name:<name> item_name:<item>", t_string!(i18n, bot_cmd_list_remove_item_desc).to_string()),
                        ("/ffxiv list show_list list_name:<name>", t_string!(i18n, bot_cmd_list_show_list_desc).to_string()),
                    ]
                />

                <CommandGroup
                    title="/ffxiv analyze"
                    description=t_string!(i18n, bot_cmd_analyze_description).to_string()
                    commands=vec![
                        ("/ffxiv analyze profit world:<name> [minimum_profit=10000] [number_recently_sold=5] [threshold_days=7]", t_string!(i18n, bot_cmd_analyze_profit_desc).to_string()),
                    ]
                />

                <CommandGroup
                    title="/ffxiv character"
                    description=t_string!(i18n, bot_cmd_character_description).to_string()
                    commands=vec![
                        ("/ffxiv character register name:<First Last> [home_world]", t_string!(i18n, bot_cmd_character_register_desc).to_string()),
                    ]
                />
            </section>
        </div>
    }
}

#[component]
fn CommandGroup(
    title: &'static str,
    #[prop(into)] description: String,
    commands: Vec<(&'static str, String)>,
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
