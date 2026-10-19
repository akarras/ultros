//! Private, resumable retainer setup. Each confirmed change is saved before
//! offering the next optional step; the session itself is only UI state.

use std::time::{Duration, Instant};

use anyhow::anyhow;
use futures::StreamExt;
use poise::{Modal, serenity_prelude as serenity};
use ultros_db::entity::{
    final_fantasy_character::Model as Character, owned_retainers::Model as Owned,
    retainer::Model as Retainer,
};

use super::retainer_services::{LiveServices, RetainerServices};
use super::{Context, Error, helpers::truncate_100};

const PAGE_SIZE: usize = 20;
const SESSION_DURATION: Duration = Duration::from_secs(10 * 60);
const IDLE_DURATION: Duration = Duration::from_secs(3 * 60);

#[derive(Clone, Debug, Default, poise::Modal)]
#[name = "Find a retainer"]
struct Search {
    #[name = "Retainer name"]
    #[max_length = 100]
    name: String,
    #[name = "World (blank searches all worlds)"]
    #[max_length = 100]
    world: Option<String>,
}

#[derive(Clone, Debug, poise::Modal)]
#[name = "Add a character"]
struct CharacterSearch {
    #[name = "Character name (First Last)"]
    #[max_length = 100]
    name: String,
    #[name = "Home world (optional)"]
    #[max_length = 100]
    world: Option<String>,
}

impl Search {
    fn bounded(name: String, world: Option<String>) -> Self {
        Self {
            name: truncate_100(&name),
            world: world.map(|world| truncate_100(&world)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Search,
    Saved,
    Listings,
    Characters,
    CharacterResults,
    List,
    Remove,
    Alerts,
}

enum Step {
    Done,
    Updated,
    Unchanged,
}

enum Dialog<T> {
    Submitted(T),
    Interrupted,
    TimedOut,
}

impl View {
    fn allows(self, action: &str) -> bool {
        match self {
            Self::Search => matches!(
                action,
                "search" | "all_worlds" | "pick_retainer" | "list" | "done" | "switch_character"
            ),
            Self::Saved => matches!(
                action,
                "assign" | "another" | "list" | "alerts" | "done" | "remove" | "listings"
            ),
            Self::Listings => matches!(action, "back" | "another" | "done"),
            Self::Characters => matches!(
                action,
                "pick_character" | "new_character" | "previous" | "next" | "back"
            ),
            Self::CharacterResults => matches!(
                action,
                "pick_new_character" | "new_character" | "assign" | "back"
            ),
            Self::List => matches!(
                action,
                "manage_retainer" | "another" | "previous" | "next" | "alerts" | "done" | "list"
            ),
            Self::Remove => matches!(action, "confirm_remove" | "back"),
            Self::Alerts => matches!(action, "list" | "done"),
        }
    }
}

struct Session {
    view: View,
    search: Search,
    matches: Vec<Retainer>,
    character_matches: Vec<lodestone::search::ProfileSearchResult>,
    character_search: Option<CharacterSearch>,
    characters: Vec<Character>,
    retainers: Vec<(Owned, Retainer)>,
    list_loaded: bool,
    listing_text: String,
    current: Option<(Owned, Retainer)>,
    // Deliberately per-session. A fresh add command never inherits a hidden
    // character or world from an earlier command.
    adding_for: Option<Character>,
    page: usize,
    notice: String,
    revision: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            view: View::Search,
            search: Search::default(),
            matches: vec![],
            character_matches: vec![],
            character_search: None,
            characters: vec![],
            retainers: vec![],
            list_loaded: false,
            listing_text: String::new(),
            current: None,
            adding_for: None,
            page: 0,
            notice: String::new(),
            revision: 0,
        }
    }
}

fn button(id: &str, label: &str) -> serenity::CreateButton {
    serenity::CreateButton::new(id)
        .label(label)
        .style(serenity::ButtonStyle::Secondary)
}

fn buttons(revision: u64, items: &[(&str, &str)]) -> serenity::CreateActionRow {
    serenity::CreateActionRow::Buttons(
        items
            .iter()
            .map(|(id, label)| button(&format!("{revision}:{id}"), label))
            .collect(),
    )
}

fn select(
    revision: u64,
    id: &str,
    placeholder: &str,
    options: Vec<serenity::CreateSelectMenuOption>,
) -> serenity::CreateActionRow {
    serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            format!("{revision}:{id}"),
            serenity::CreateSelectMenuKind::String { options },
        )
        .placeholder(placeholder),
    )
}

/// Values must have come from the currently displayed page, not merely be
/// parseable IDs. Also used for character results that have no DB row yet.
fn selected_id(interaction: &serenity::ComponentInteraction) -> Result<i32, Error> {
    match &interaction.data.kind {
        serenity::ComponentInteractionDataKind::StringSelect { values } if values.len() == 1 => {
            Ok(values[0].parse()?)
        }
        _ => Err(anyhow!("Choose one of the options in this menu.").into()),
    }
}

fn page_slice<T>(items: &[T], page: usize) -> &[T] {
    let start = page.saturating_mul(PAGE_SIZE).min(items.len());
    &items[start..(start + PAGE_SIZE).min(items.len())]
}

fn unique_exact_match<'a>(name: &str, matches: &'a [Retainer]) -> Option<&'a Retainer> {
    (matches.len() == 1 && matches[0].name.eq_ignore_ascii_case(name.trim())).then(|| &matches[0])
}

impl Session {
    async fn start_add(
        services: &impl RetainerServices,
        retainer: Option<String>,
        world: Option<String>,
        character: Option<i32>,
    ) -> Result<Self, Error> {
        let mut session = Self {
            search: Search::bounded(retainer.clone().unwrap_or_default(), world),
            ..Default::default()
        };
        // Character details are optional. A failed lookup must not prevent a
        // retainer from being tracked or discard the recoverable setup menu.
        let character_error = session.refresh_characters(services).await.err();
        session.adding_for = character
            .and_then(|id| session.characters.iter().find(|c| c.id == id))
            .cloned();
        let missing_character =
            character_error.is_none() && character.is_some() && session.adding_for.is_none();
        // Autocomplete values use an explicit prefix so an arbitrary typed name
        // cannot accidentally be interpreted as a database primary key.
        let selected = retainer
            .as_deref()
            .and_then(|r| r.strip_prefix("retainer:"))
            .and_then(|r| r.parse().ok());
        if selected.is_some() {
            session.search.name.clear();
        }
        if let Err(error) = session.search(services, selected).await {
            session.notice = error.to_string();
        }
        if let Some(error) = character_error {
            session.notice.push_str(&format!("\n{error}\nI couldn't load your characters. You can still track retainers and retry Assign / change character later."));
        }
        if missing_character {
            session.notice.push_str("\nThat character is no longer on your account. You can still track the retainer and use Assign / change character to choose or add another.");
        }
        Ok(session)
    }

    async fn start_list(services: &impl RetainerServices) -> Self {
        let mut session = Self::default();
        if let Err(error) = session.load_list(services).await {
            session.notice = error.to_string();
        }
        session
    }

    fn action<'a>(&self, custom_id: &'a str) -> Option<&'a str> {
        let (revision, action) = custom_id.split_once(':')?;
        (revision.parse::<u64>().ok() == Some(self.revision) && self.view.allows(action))
            .then_some(action)
    }

    async fn refresh_characters(&mut self, services: &impl RetainerServices) -> Result<(), Error> {
        self.characters = services.characters().await?;
        self.characters
            .sort_by_key(|c| (c.first_name.clone(), c.last_name.clone(), c.world_id, c.id));
        Ok(())
    }

    async fn search(
        &mut self,
        services: &impl RetainerServices,
        selected: Option<i32>,
    ) -> Result<(), Error> {
        self.view = View::Search;
        self.matches.clear();
        if let Some(id) = selected {
            let retainer = services.retainer(id).await?.ok_or_else(|| {
                anyhow!("That retainer is no longer available. Edit your search to try again.")
            })?;
            self.search.name.clone_from(&retainer.name);
            let world = services.world_id(self.search.world.as_deref())?;
            if world.is_some_and(|world| retainer.world_id != world) {
                return Err(anyhow!("That choice is on a different world. Edit the search or search all worlds to choose the right retainer.").into());
            }
            return self.save(services, retainer).await;
        }
        let world = services.world_id(self.search.world.as_deref())?;
        if self.search.name.trim().is_empty() {
            return Ok(());
        }
        self.matches = services.search(&self.search.name, world).await?;
        // Only a unique exact name may bypass the chooser. A single prefix
        // result still needs confirmation, just like ambiguous exact names.
        if let Some(retainer) = unique_exact_match(&self.search.name, &self.matches) {
            return self.save(services, retainer.clone()).await;
        }
        if self.matches.is_empty() {
            self.notice = "No matching retainer in Ultros's market data. Check the spelling or search all worlds. If it hasn't appeared in market data yet, list an item and try again after that market data updates.".into();
        }
        Ok(())
    }

    async fn save(
        &mut self,
        services: &impl RetainerServices,
        retainer: Retainer,
    ) -> Result<(), Error> {
        let (owned, created) = services.save(retainer.id).await?;
        self.current = Some((owned.clone(), retainer));
        self.view = View::Saved;
        self.notice = if created {
            "Retainer added. You can organize it now or later."
        } else {
            "You're already tracking this retainer. Its character assignment has been kept."
        }
        .into();
        if !created {
            self.adding_for = owned
                .character_id
                .and_then(|id| self.characters.iter().find(|c| c.id == id))
                .cloned();
        }
        // Send for retries too: a previous save may have succeeded even when
        // its event or Discord reply was lost. The listener resyncs this owner
        // while preserving alert state for unchanged listings.
        services.retainer_added(owned);
        if created && let Some(character) = self.adding_for.clone() {
            self.assign(services, Some(character)).await?;
        }
        Ok(())
    }

    async fn assign(
        &mut self,
        services: &impl RetainerServices,
        character: Option<Character>,
    ) -> Result<(), Error> {
        let (owned, retainer) = self
            .current
            .as_ref()
            .ok_or_else(|| anyhow!("Choose a retainer first."))?;
        if let Some(character) = &character
            && character.world_id != retainer.world_id
        {
            self.view = View::Characters;
            self.page = 0;
            return Err(anyhow!(
                "{} is on {}, but this retainer is on {}. Choose a character on the retainer's world or keep it unassigned. The retainer is still tracked.",
                character.first_name,
                services.world_name(character.world_id),
                services.world_name(retainer.world_id)
            ).into());
        }
        services
            .assign(owned.id, character.as_ref().map(|c| c.id))
            .await?;
        self.current.as_mut().unwrap().0.character_id = character.as_ref().map(|c| c.id);
        self.adding_for = character;
        self.view = View::Saved;
        self.notice = "Character assignment saved.".into();
        Ok(())
    }

    async fn load_list(&mut self, services: &impl RetainerServices) -> Result<(), Error> {
        self.view = View::List;
        self.page = 0;
        self.retainers.clear();
        self.list_loaded = false;
        self.retainers = services.retainers().await?;
        self.refresh_characters(services).await?;
        self.list_loaded = true;
        Ok(())
    }

    async fn find_characters(
        &mut self,
        services: &impl RetainerServices,
        mut search: CharacterSearch,
    ) -> Result<(), Error> {
        search.name = truncate_100(&search.name);
        search.world = search.world.map(|world| truncate_100(&world));
        self.character_search = Some(search.clone());
        self.view = View::CharacterResults;
        self.character_matches.clear();
        self.character_matches = services
            .search_characters(&search.name, search.world.as_deref())
            .await?;
        Ok(())
    }

    /// Shared by the gateway loop and tests so failed actions produce the same
    /// recovery message and stale-control revision in both environments.
    fn finish_step(&mut self, result: Result<Step, Error>) -> Step {
        match result {
            Ok(Step::Done) => return Step::Done,
            Ok(Step::Unchanged) => return Step::Unchanged,
            Ok(Step::Updated) => {}
            Err(error) => {
                tracing::warn!(?error, "Discord retainer setup action failed");
                self.notice = format!(
                    "{error}\nYour completed changes have been kept. You can retry or run `/ffxiv retainer list` to continue."
                );
            }
        }
        self.revision += 1;
        Step::Updated
    }

    fn reply(&self, world: impl Fn(i32) -> String) -> poise::CreateReply {
        let character_name = |c: &Character| {
            format!(
                "{} {} \u{00b7} {}",
                c.first_name,
                c.last_name,
                world(c.world_id)
            )
        };
        let mut components = vec![];
        let (title, mut description) = match self.view {
            View::Search => {
                if !self.matches.is_empty() {
                    components.push(select(
                        self.revision,
                        "pick_retainer",
                        "Choose your retainer and world",
                        self.matches
                            .iter()
                            .take(25)
                            .map(|r| {
                                serenity::CreateSelectMenuOption::new(
                                    truncate_100(&format!("{} · {}", r.name, world(r.world_id))),
                                    r.id.to_string(),
                                )
                            })
                            .collect(),
                    ));
                }
                components.push(buttons(
                    self.revision,
                    &[
                        (
                            "search",
                            if self.search.name.is_empty() {
                                "Search by name"
                            } else {
                                "Edit search"
                            },
                        ),
                        ("all_worlds", "Search all worlds"),
                        ("list", "My retainers"),
                        ("done", "Done"),
                    ],
                ));
                if self.adding_for.is_some() {
                    components.push(buttons(
                        self.revision,
                        &[("switch_character", "Switch character")],
                    ));
                }
                let world = self.search.world.as_deref().unwrap_or("All worlds");
                let name = if self.search.name.is_empty() {
                    "Not entered"
                } else {
                    &self.search.name
                };
                let mut text = format!(
                    "Search: **{}**\nWorld: **{world}**\n\nChoose a retainer from the results, or search by name. A character is optional.",
                    name
                );
                if self.matches.len() > 25 {
                    text.push_str("\n\nMore than 25 matches. Add more of the name or choose a world to narrow the results.");
                }
                ("Find a retainer", text)
            }
            View::Saved | View::Remove => {
                let (owned, retainer) = self.current.as_ref().expect("saved view has a retainer");
                let character = owned
                    .character_id
                    .and_then(|id| self.characters.iter().find(|c| c.id == id))
                    .map(character_name)
                    .unwrap_or_else(|| {
                        if owned.character_id.is_some() {
                            "Character details unavailable".into()
                        } else {
                            "Not assigned".into()
                        }
                    });
                let text = format!(
                    "Retainer: **{} · {}**\nCharacter: **{character}**\n\nThis retainer is tracked on your account.",
                    retainer.name,
                    world(retainer.world_id)
                );
                if self.view == View::Remove {
                    components.push(buttons(
                        self.revision,
                        &[
                            ("confirm_remove", "Remove from my account"),
                            ("back", "Cancel"),
                        ],
                    ));
                    ("Remove this retainer?", text)
                } else {
                    components.push(buttons(
                        self.revision,
                        &[
                            ("assign", "Assign / change character"),
                            ("another", "Add another"),
                            ("list", "My retainers"),
                            ("alerts", "Set up alerts"),
                            ("done", "Done"),
                        ],
                    ));
                    components.push(buttons(
                        self.revision,
                        &[("listings", "View listings"), ("remove", "Remove retainer")],
                    ));
                    ("Your retainer", text)
                }
            }
            View::Listings => {
                components.push(buttons(
                    self.revision,
                    &[
                        ("back", "Back"),
                        ("another", "Add another"),
                        ("done", "Done"),
                    ],
                ));
                let (_, retainer) = self.current.as_ref().expect("listings view has a retainer");
                (
                    "Retainer listings",
                    format!(
                        "**{} · {}**\n\n{}",
                        retainer.name,
                        world(retainer.world_id),
                        self.listing_text
                    ),
                )
            }
            View::Characters => {
                let mut options = vec![serenity::CreateSelectMenuOption::new("Not assigned", "0")];
                options.extend(page_slice(&self.characters, self.page).iter().map(|c| {
                    serenity::CreateSelectMenuOption::new(
                        truncate_100(&character_name(c)),
                        c.id.to_string(),
                    )
                }));
                components.push(select(
                    self.revision,
                    "pick_character",
                    "Choose a character, or leave unassigned",
                    options,
                ));
                components.push(buttons(
                    self.revision,
                    &[
                        ("new_character", "Add a character"),
                        ("previous", "Previous"),
                        ("next", "Next"),
                        ("back", "Back"),
                    ],
                ));
                (
                    "Group this retainer",
                    format!(
                        "Choose who this retainer belongs to. Characters only organize your retainers.\n\nPage {} of {}. You can add another character here without leaving Discord.",
                        self.page + 1,
                        self.characters.len().div_ceil(PAGE_SIZE).max(1)
                    ),
                )
            }
            View::CharacterResults => {
                if !self.character_matches.is_empty() {
                    components.push(select(
                        self.revision,
                        "pick_new_character",
                        "Choose the matching Lodestone character",
                        self.character_matches
                            .iter()
                            .take(25)
                            .map(|c| {
                                serenity::CreateSelectMenuOption::new(
                                    truncate_100(&format!("{} · {}", c.name, c.world)),
                                    c.user_id.to_string(),
                                )
                            })
                            .collect(),
                    ));
                }
                components.push(buttons(
                    self.revision,
                    &[
                        ("new_character", "Edit character search"),
                        ("assign", "Existing characters"),
                        ("back", "Keep retainer as it is"),
                    ],
                ));
                ("Add a character", "Choose the matching name and home world. No Lodestone bio challenge is needed. If your character isn't listed, narrow the search by name and world. Your retainer stays tracked either way.".into())
            }
            View::List if !self.list_loaded => {
                components.push(buttons(
                    self.revision,
                    &[("list", "Retry loading retainers"), ("done", "Done")],
                ));
                ("Your retainers", "I couldn't load your retainer list. Your completed changes have been kept; try loading it again.".into())
            }
            View::List => {
                let page = page_slice(&self.retainers, self.page);
                if !page.is_empty() {
                    components.push(select(
                        self.revision,
                        "manage_retainer",
                        "Choose a retainer to organize or remove",
                        page.iter()
                            .map(|(o, r)| {
                                serenity::CreateSelectMenuOption::new(
                                    truncate_100(&format!("{} · {}", r.name, world(r.world_id))),
                                    o.id.to_string(),
                                )
                            })
                            .collect(),
                    ));
                }
                components.push(buttons(
                    self.revision,
                    &[
                        ("another", "Add a retainer"),
                        ("previous", "Previous"),
                        ("next", "Next"),
                        ("alerts", "Set up alerts"),
                        ("done", "Done"),
                    ],
                ));
                let mut text = format!(
                    "{} retainers tracked · Page {} of {}\n",
                    self.retainers.len(),
                    self.page + 1,
                    self.retainers.len().div_ceil(PAGE_SIZE).max(1)
                );
                let mut previous_group = None;
                for (owned, retainer) in page {
                    let group = owned
                        .character_id
                        .and_then(|id| self.characters.iter().find(|c| c.id == id))
                        .map(|c| format!("{} {}", c.first_name, c.last_name))
                        .unwrap_or_else(|| "Unassigned".into());
                    if previous_group != Some(owned.character_id) {
                        text.push_str(&format!("\n\n**{group}**"));
                        previous_group = Some(owned.character_id);
                    }
                    text.push_str(&format!(
                        "\n{} · {}",
                        retainer.name,
                        world(retainer.world_id)
                    ));
                }
                if page.is_empty() {
                    text.push_str(
                        "\nAdd your first retainer. You don't need to add a character first.",
                    );
                }
                ("Your retainers", text)
            }
            View::Alerts => {
                components.push(buttons(
                    self.revision,
                    &[("list", "My retainers"), ("done", "Done")],
                ));
                ("Choose where alerts go", "Alerts cover **all your tracked retainers**, including every character and world.\n\nRun one of these commands in the channel where you want alerts, or in a DM with the bot for private alerts:\n\n`/ffxiv retainer add_undercut_alert margin_percent:0`\n`/ffxiv retainer add_sale_alert`\n\nNo alerts have been enabled by this setup menu.".into())
            }
        };
        if let Some(character) = &self.adding_for
            && !matches!(self.view, View::Alerts | View::List)
        {
            description.push_str(&format!("\n\nAdding for: **{}**. You can switch characters in this menu. A fresh add command starts without a character.", character_name(character)));
        }
        if !self.notice.is_empty() {
            description.push_str(&format!("\n\n{}", self.notice));
        }
        poise::CreateReply::default()
            .ephemeral(true)
            .allowed_mentions(serenity::CreateAllowedMentions::new())
            .embed(
                serenity::CreateEmbed::new()
                    .title(title)
                    .description(description)
                    .color(super::ULTROS_COLOR),
            )
            .components(components)
    }

    async fn handle(
        &mut self,
        ctx: Context<'_>,
        interaction: serenity::ComponentInteraction,
        remaining: Duration,
    ) -> Result<Step, Error> {
        let services = LiveServices(ctx);
        let Some(action) = self.action(&interaction.data.custom_id) else {
            interaction.defer(ctx.serenity_context()).await?;
            return Err(
                anyhow!("This menu has changed. Use its latest buttons and choices.").into(),
            );
        };
        if action == "search" || action == "new_character" {
            if action == "search" {
                match modal::<Search>(ctx, interaction, Some(self.search.clone()), remaining).await? {
                    Dialog::Submitted(search) => {
                        self.search = Search::bounded(search.name, search.world);
                        self.search(&services, None).await?;
                    }
                    Dialog::Interrupted => return Ok(Step::Unchanged),
                    Dialog::TimedOut => self.notice = "The search dialog timed out. You can open it again; your saved changes are safe.".into(),
                }
            } else {
                match modal::<CharacterSearch>(ctx, interaction, self.character_search.clone(), remaining).await? {
                    Dialog::Submitted(search) => {
                        self.find_characters(&services, search).await?;
                        if self.character_matches.is_empty() {
                            self.notice = "No matching characters found. Check the name and home world, or keep the retainer unassigned.".into();
                        }
                    }
                    Dialog::Interrupted => return Ok(Step::Unchanged),
                    Dialog::TimedOut => self.notice = "The character dialog timed out. You can open it again; your saved changes are safe.".into(),
                }
            }
            return Ok(Step::Updated);
        }
        // Acknowledge before any database or Lodestone work.
        interaction.defer(ctx.serenity_context()).await?;
        let selected = if matches!(
            &interaction.data.kind,
            serenity::ComponentInteractionDataKind::StringSelect { .. }
        ) {
            Some(selected_id(&interaction)?)
        } else {
            None
        };
        self.dispatch(&services, &interaction.data.custom_id, selected)
            .await
    }

    /// Command actions independent of Discord's transport and interaction tokens.
    async fn dispatch(
        &mut self,
        services: &impl RetainerServices,
        custom_id: &str,
        selected: Option<i32>,
    ) -> Result<Step, Error> {
        let action = self
            .action(custom_id)
            .ok_or_else(|| anyhow!("This menu has changed. Use its latest buttons and choices."))?;
        match action {
            "done" => return Ok(Step::Done),
            "listings" => {
                let (_, retainer) = self
                    .current
                    .as_ref()
                    .ok_or_else(|| anyhow!("Choose a retainer first."))?;
                let listings = services.listings(retainer.id).await?;
                let language = super::helpers::discord_locale_to_xiv_language(services.locale());
                self.listing_text = listings
                    .iter()
                    .take(20)
                    .map(|listing| {
                        let name = super::helpers::localized_item_name(listing.item_id, language);
                        let quality = if listing.hq { " (HQ)" } else { "" };
                        format!(
                            "{}{}: {} × {} gil",
                            truncate_100(&name),
                            quality,
                            listing.quantity,
                            listing.price_per_unit
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if listings.is_empty() {
                    self.listing_text =
                        "No active listings in Ultros's current market data.".into();
                }
                if listings.len() > 20 {
                    self.listing_text
                        .push_str("\nShowing the first 20 listings.");
                }
                self.view = View::Listings;
            }
            "all_worlds" => {
                self.search.world = None;
                self.search(services, None).await?;
            }
            "pick_retainer" => {
                let id =
                    selected.ok_or_else(|| anyhow!("Choose one of the options in this menu."))?;
                let retainer = self
                    .matches
                    .iter()
                    .take(25)
                    .find(|r| r.id == id)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow!("That result is no longer in this menu. Search again.")
                    })?;
                self.save(services, retainer).await?;
            }
            "assign" | "switch_character" => {
                self.view = View::Characters;
                self.page = 0;
                self.refresh_characters(services).await?;
            }
            "pick_character" => {
                let id =
                    selected.ok_or_else(|| anyhow!("Choose one of the options in this menu."))?;
                let character = if id == 0 {
                    None
                } else {
                    Some(
                        page_slice(&self.characters, self.page)
                            .iter()
                            .find(|c| c.id == id)
                            .cloned()
                            .ok_or_else(|| anyhow!("Choose a character from this page."))?,
                    )
                };
                if self.current.is_some() {
                    self.assign(services, character).await?;
                } else {
                    self.search.world = character.as_ref().map(|c| services.world_name(c.world_id));
                    self.adding_for = character;
                    self.matches.clear();
                    self.view = View::Search;
                }
            }
            "pick_new_character" => {
                let id =
                    selected.ok_or_else(|| anyhow!("Choose one of the options in this menu."))?;
                if !self
                    .character_matches
                    .iter()
                    .take(25)
                    .any(|c| c.user_id == id as u32)
                {
                    return Err(anyhow!("Choose a character from this search.").into());
                }
                let character = services.claim_character(id as u32).await?;
                self.refresh_characters(services).await?;
                if self.current.is_some() {
                    self.assign(services, Some(character)).await?;
                } else {
                    self.search.world = Some(services.world_name(character.world_id));
                    self.adding_for = Some(character);
                    self.matches.clear();
                    self.view = View::Search;
                    self.notice =
                        "Character added. You can now add a retainer for this character.".into();
                }
            }
            "another" => {
                self.current = None;
                self.matches.clear();
                self.search = Search {
                    name: String::new(),
                    world: self
                        .adding_for
                        .as_ref()
                        .map(|c| services.world_name(c.world_id)),
                };
                self.view = View::Search;
            }
            "list" => {
                self.adding_for = None;
                self.load_list(services).await?;
            }
            "manage_retainer" => {
                let id =
                    selected.ok_or_else(|| anyhow!("Choose one of the options in this menu."))?;
                self.current = Some(
                    page_slice(&self.retainers, self.page)
                        .iter()
                        .find(|(o, _)| o.id == id)
                        .cloned()
                        .ok_or_else(|| anyhow!("Choose a retainer from this page."))?,
                );
                self.adding_for = self
                    .current
                    .as_ref()
                    .and_then(|(o, _)| o.character_id)
                    .and_then(|id| self.characters.iter().find(|c| c.id == id))
                    .cloned();
                self.view = View::Saved;
            }
            "previous" => self.page = self.page.saturating_sub(1),
            "next" => {
                let len = if self.view == View::List {
                    self.retainers.len()
                } else {
                    self.characters.len()
                };
                if (self.page + 1) * PAGE_SIZE < len {
                    self.page += 1;
                }
            }
            "remove" => self.view = View::Remove,
            "confirm_remove" => {
                let (owned, _) = self
                    .current
                    .as_ref()
                    .ok_or_else(|| anyhow!("Choose a retainer first."))?;
                services.remove(owned.id).await?;
                self.current = None;
                self.adding_for = None;
                self.load_list(services).await?;
                self.notice = "Retainer removed from your account.".into();
            }
            "back" => {
                self.view = if self.current.is_some() {
                    View::Saved
                } else {
                    View::Search
                }
            }
            "alerts" => self.view = View::Alerts,
            _ => {
                return Err(anyhow!(
                    "This control is no longer available. Run /ffxiv retainer list to continue."
                )
                .into());
            }
        }
        Ok(Step::Updated)
    }

    async fn run(mut self, ctx: Context<'_>) -> Result<(), Error> {
        let reply = ctx
            .send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content("Loading your retainer setup..."),
            )
            .await?;
        let message = reply.message().await?.into_owned();
        let deadline = Instant::now() + SESSION_DURATION;
        let interactions = message
            .await_component_interaction(ctx.serenity_context())
            .author_id(ctx.author().id)
            .timeout(SESSION_DURATION)
            .stream();
        futures::pin_mut!(interactions);
        reply
            .edit(
                ctx,
                self.reply(|id| LiveServices(ctx).world_name(id))
                    .content(""),
            )
            .await?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let interaction =
                tokio::time::timeout(remaining.min(IDLE_DURATION), interactions.next()).await;
            let Ok(Some(interaction)) = interaction else {
                break;
            };
            self.notice.clear();
            let result = self
                .handle(
                    ctx,
                    interaction,
                    deadline.saturating_duration_since(Instant::now()),
                )
                .await;
            match self.finish_step(result) {
                Step::Done => {
                    reply.edit(ctx, poise::CreateReply::default().content("Your changes are saved. Run `/ffxiv retainer list` whenever you want to manage your retainers.").components(vec![])).await?;
                    return Ok(());
                }
                Step::Unchanged => continue,
                Step::Updated => {}
            }
            reply
                .edit(ctx, self.reply(|id| LiveServices(ctx).world_name(id)))
                .await?;
        }
        reply.edit(ctx, poise::CreateReply::default().content("This setup menu has expired. Your saved retainers are still tracked. Run `/ffxiv retainer list` to continue.").components(vec![])).await?;
        Ok(())
    }
}

/// Install the collector before sending the modal, and scope submissions to
/// both this interaction and this player. A modal timeout never changes DB state.
async fn modal<M: Modal>(
    ctx: Context<'_>,
    interaction: serenity::ComponentInteraction,
    defaults: Option<M>,
    remaining: Duration,
) -> Result<Dialog<M>, Error> {
    let custom_id = interaction.id.to_string();
    let id = custom_id.clone();
    let collector = serenity::ModalInteractionCollector::new(ctx.serenity_context())
        .author_id(ctx.author().id)
        .filter(move |m| m.data.custom_id == id)
        .timeout(remaining.min(IDLE_DURATION))
        .stream();
    // Discord emits no modal-cancel event. Both collectors receive a later
    // menu click: interrupt this wait, then let the persistent session queue
    // handle that click with the same render revision.
    let trigger = interaction.id;
    let menu = serenity::ComponentInteractionCollector::new(ctx.serenity_context())
        .author_id(ctx.author().id)
        .message_id(interaction.message.id)
        .filter(move |click| click.id != trigger)
        .timeout(remaining.min(IDLE_DURATION))
        .stream();
    interaction
        .create_response(ctx.serenity_context(), M::create(defaults, custom_id))
        .await?;
    futures::pin_mut!(collector);
    futures::pin_mut!(menu);
    tokio::select! {
        response = collector.next() => {
            if let Some(response) = response {
                response.defer(ctx.serenity_context()).await?;
                Ok(Dialog::Submitted(M::parse(response.data.clone()).map_err(|error| anyhow!(error))?))
            } else { Ok(Dialog::TimedOut) }
        }
        click = menu.next() => {
            Ok(if click.is_some() { Dialog::Interrupted } else { Dialog::TimedOut })
        }
    }
}

pub(super) async fn add(
    ctx: Context<'_>,
    retainer: Option<String>,
    world: Option<String>,
    character: Option<i32>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let session = Session::start_add(&LiveServices(ctx), retainer, world, character).await?;
    session.run(ctx).await
}

pub(super) async fn list(ctx: Context<'_>) -> Result<(), Error> {
    if matches!(ctx, poise::Context::Prefix(_)) {
        ctx.say("Use `/ffxiv retainer list` to open your private retainer menu.")
            .await?;
        return Ok(());
    }
    ctx.defer_ephemeral().await?;
    let session = Session::start_list(&LiveServices(ctx)).await;
    session.run(ctx).await
}

pub(super) async fn register_character(
    ctx: Context<'_>,
    name: String,
    world: Option<String>,
) -> Result<(), Error> {
    if matches!(ctx, poise::Context::Prefix(_)) {
        ctx.say("Use `/ffxiv character register` to add a character privately, or `/ffxiv retainer add` to start with a retainer.").await?;
        return Ok(());
    }
    ctx.defer_ephemeral().await?;
    let mut session = Session::default();
    match session
        .find_characters(&LiveServices(ctx), CharacterSearch { name, world })
        .await
    {
        Ok(()) => {
            if session.character_matches.is_empty() {
                session.notice =
                    "No matching characters found. Edit the name or home world to try again."
                        .into();
            }
        }
        Err(error) => {
            session.notice = error.to_string();
        }
    }
    session.run(ctx).await
}

#[cfg(test)]
#[path = "retainer_setup_tests.rs"]
mod command_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn retainer(id: i32, world_id: i32) -> Retainer {
        Retainer {
            id,
            world_id,
            name: "Dawnkeeper".into(),
            retainer_city_id: 1,
        }
    }

    fn owned(id: i32, character_id: Option<i32>) -> Owned {
        Owned {
            id,
            retainer_id: id,
            discord_id: 100,
            character_id,
            weight: None,
        }
    }

    #[test]
    fn typed_names_require_a_unique_exact_match() {
        let candidates = [retainer(1, 10), retainer(2, 20)];
        assert!(
            unique_exact_match("Dawnkeeper", &candidates).is_none(),
            "same name on two worlds needs a chooser"
        );
        assert!(
            unique_exact_match("Dawn", &candidates[..1]).is_none(),
            "a prefix alone must not claim a retainer"
        );
        assert_eq!(
            unique_exact_match(" dawnKEEPER ", &candidates[..1])
                .unwrap()
                .id,
            1
        );
        assert!(unique_exact_match("Missing", &[]).is_none());
    }

    #[test]
    fn stale_or_unoffered_actions_cannot_remove_a_different_retainer() {
        let mut session = Session {
            revision: 7,
            view: View::Saved,
            ..Default::default()
        };
        assert_eq!(session.action("7:remove"), Some("remove"));
        assert!(session.action("7:confirm_remove").is_none());
        session.view = View::Remove;
        assert_eq!(session.action("7:confirm_remove"), Some("confirm_remove"));
        session.revision += 1;
        assert!(
            session.action("7:confirm_remove").is_none(),
            "confirmation from an old render is stale"
        );
        session.view = View::Search;
        assert!(session.action("8:confirm_remove").is_none());
        assert!(session.action("not-a-control").is_none());
    }

    #[test]
    fn every_menu_fits_discord_limits_and_setup_is_private() {
        let mut session = Session {
            current: Some((owned(1, Some(1)), retainer(1, 10))),
            list_loaded: true,
            ..Default::default()
        };
        for id in 1..=60 {
            session.characters.push(Character {
                id,
                world_id: 10,
                first_name: "Player".into(),
                last_name: format!("Alt{id}"),
            });
            session
                .retainers
                .push((owned(id, Some(id)), retainer(id, 10)));
            session
                .character_matches
                .push(lodestone::search::ProfileSearchResult {
                    user_id: id as u32,
                    name: format!("Player Alt{id}"),
                    world: "Gilgamesh".into(),
                });
        }
        session.matches = (1..=26).map(|id| retainer(id, 10)).collect();
        for view in [
            View::Search,
            View::Saved,
            View::Listings,
            View::Characters,
            View::CharacterResults,
            View::List,
            View::Remove,
            View::Alerts,
        ] {
            session.view = view;
            let reply = session.reply(|id| format!("World {id}"));
            assert_eq!(reply.ephemeral, Some(true));
            let embed = serde_json::to_value(&reply.embeds[0]).unwrap();
            assert!(embed["description"].as_str().unwrap().chars().count() <= 4096);
            let rows = serde_json::to_value(reply.components.unwrap()).unwrap();
            assert!(rows.as_array().unwrap().len() <= 5);
            for row in rows.as_array().unwrap() {
                assert!(row["components"].as_array().unwrap().len() <= 5);
                for component in row["components"].as_array().unwrap() {
                    assert!(
                        session
                            .action(component["custom_id"].as_str().unwrap())
                            .is_some(),
                        "every displayed control is valid for this view"
                    );
                    if let Some(options) = component.get("options") {
                        let options = options.as_array().unwrap();
                        assert!(!options.is_empty() && options.len() <= 25);
                        for option in options {
                            assert!(option["label"].as_str().unwrap().chars().count() <= 100);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fresh_commands_have_no_hidden_character_and_lists_include_unassigned() {
        let mut session = Session::default();
        assert!(session.adding_for.is_none());
        assert!(session.search.world.is_none());
        session.view = View::List;
        session.list_loaded = true;
        session.retainers = vec![(owned(1, None), retainer(1, 20))];
        let reply = session.reply(|id| format!("World {id}"));
        let embed = serde_json::to_value(&reply.embeds[0]).unwrap();
        let text = embed["description"].as_str().unwrap();
        assert!(text.contains("Unassigned"));
        assert!(text.contains("Dawnkeeper") && text.contains("World 20"));
        let items: Vec<_> = (0..61).collect();
        assert_eq!(page_slice(&items, 3), [60]);
        assert!(page_slice(&items, usize::MAX).is_empty());
    }
}
