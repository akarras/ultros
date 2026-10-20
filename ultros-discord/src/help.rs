//! `/help` rendering.
//!
//! poise 0.7 removed `builtins::help()` and `builtins::pretty_help()`, so the
//! command's output is produced here instead. This is a port of poise 0.6.2's
//! `builtins::help` (the version this bot shipped against) narrowed to what
//! Ultros actually uses, so the rendered message is unchanged:
//!
//! - no context-menu commands exist in this bot, so the context-menu listing
//!   and `show_context_menu_commands` are gone;
//! - no command sets `category`, so every command lands under the single
//!   `Commands:` heading 0.6 used as the fallback;
//! - `prefix_options.prefix` is never configured and every command registers a
//!   `slash_action`, so the invocation prefix is always `/` and the dynamic
//!   prefix callback 0.6 consulted cannot fire;
//! - replies stay ephemeral, matching `HelpConfiguration::default()`.
//!
//! The per-command view keeps 0.6's layout: bolded invocation, description and
//! help text, then the aligned `Parameters:` and `Subcommands:` code blocks.

use std::fmt::Write as _;

use poise::CreateReply;

use crate::{Data, Error};

/// Text appended to the bottom of the command overview, as 0.6's
/// `HelpConfiguration::extra_text_at_bottom` did.
const EXTRA_TEXT_AT_BOTTOM: &str = "ultros 🦑";

type Command = poise::Command<Data, Error>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// Aligns descriptions into a second column, as poise 0.6's `TwoColumnList` did.
#[derive(Default)]
struct TwoColumnList(Vec<(String, Option<String>)>);

impl TwoColumnList {
    /// Add a line that participates in the first column's width calculation.
    fn push_two_columns(&mut self, command: String, description: String) {
        self.0.push((command, Some(description)));
    }

    /// Add a heading, which does not influence the first column's width.
    fn push_heading(&mut self, category: &str) {
        if !self.0.is_empty() {
            self.0.push((String::new(), None));
        }
        self.0.push((format!("{category}:"), None));
    }

    fn into_string(self) -> String {
        let longest_command = self
            .0
            .iter()
            .filter_map(|(command, description)| description.is_some().then_some(command.len()))
            .max()
            .unwrap_or(0);
        let mut text = String::new();
        for (command, description) in self.0 {
            match description {
                Some(description) => {
                    let padding = " ".repeat(longest_command - command.len() + 3);
                    let _ = writeln!(text, "{command}{padding}{description}");
                }
                None => {
                    let _ = writeln!(text, "{command}");
                }
            }
        }
        text
    }
}

/// Render one subcommand per line. Deliberately not recursive: deeper levels
/// are shown when the user asks for help on the subcommand itself.
fn preformat_subcommands(list: &mut TwoColumnList, command: &Command, prefix: &str) {
    for subcommand in &command.subcommands {
        list.push_two_columns(
            format!("{prefix} {}", subcommand.name),
            subcommand.description.as_deref().unwrap_or("").to_string(),
        );
    }
}

/// `/help` with no argument: every visible command with its description.
fn all_commands(ctx: Context<'_>) -> String {
    let mut list = TwoColumnList::default();
    let commands = ctx
        .framework()
        .options()
        .commands
        .iter()
        .filter(|cmd| {
            !cmd.hide_in_help && (cmd.prefix_action.is_some() || cmd.slash_action.is_some())
        })
        .collect::<Vec<_>>();

    if !commands.is_empty() {
        list.push_heading("Commands");
        for command in commands {
            list.push_two_columns(
                format!("  /{}", command.name),
                command.description.as_deref().unwrap_or("").to_string(),
            );
        }
    }

    format!("```\n{}\n{EXTRA_TEXT_AT_BOTTOM}\n```", list.into_string())
}

/// `/help <command>`: detail for one command, including nested subcommands.
fn single_command(ctx: Context<'_>, command_name: &str) -> String {
    let commands = &ctx.framework().options().commands;
    // poise 0.7's `find_command` reports the matched name and the unconsumed
    // remainder, and pushes the command chain it walked into `command_tree`;
    // the command itself is the deepest entry. (0.6 returned the command.)
    let mut command_tree = Vec::new();
    let matched = poise::find_command(commands, command_name, true, &mut command_tree);
    let Some(command) = matched.and(command_tree.last().copied()) else {
        return format!("No such command `{command_name}`");
    };

    let invocation = format!("`/{}`", command.name);
    let mut text = match (&command.description, &command.help_text) {
        (Some(description), Some(help_text)) => format!("{description}\n\n{help_text}"),
        (Some(description), None) => description.to_string(),
        (None, Some(help_text)) => help_text.to_string(),
        (None, None) => "No help available".to_string(),
    };

    if !command.parameters.is_empty() {
        text += "\n\n```\nParameters:\n";
        let mut list = TwoColumnList::default();
        for parameter in &command.parameters {
            let requirement = if parameter.required {
                "required"
            } else {
                "optional"
            };
            let description = parameter.description.as_deref().unwrap_or("");
            list.push_two_columns(
                parameter.name.to_string(),
                format!("({requirement}) {description}"),
            );
        }
        text += &list.into_string();
        text += "```";
    }

    if !command.subcommands.is_empty() {
        text += "\n\n```\nSubcommands:\n";
        let mut list = TwoColumnList::default();
        preformat_subcommands(&mut list, command, &format!("  /{}", command.name));
        text += &list.into_string();
        text += "```";
    }

    format!("**{invocation}**\n\n{text}")
}

/// Send the help message, ephemeral as `HelpConfiguration::default()` was.
pub(crate) async fn help(ctx: Context<'_>, command: Option<&str>) -> Result<(), Error> {
    let content = match command {
        Some(command_name) => single_command(ctx, command_name),
        None => all_commands(ctx),
    };
    ctx.send(CreateReply::default().content(content).ephemeral(true))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TwoColumnList;

    #[test]
    fn descriptions_align_behind_the_longest_command() {
        let mut list = TwoColumnList::default();
        list.push_heading("Commands");
        list.push_two_columns("  /ping".to_string(), "Emits a ping".to_string());
        list.push_two_columns(
            "  /a-much-longer-one".to_string(),
            "Does things".to_string(),
        );

        let rendered = list.into_string();
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(lines[0], "Commands:");
        // Both descriptions start in the same column, three spaces past the
        // longest command, and no command gets less than that gap.
        assert_eq!(lines[1].find("Emits a ping"), lines[2].find("Does things"));
        assert_eq!(
            lines[2], "  /a-much-longer-one   Does things",
            "the longest command keeps exactly the three-space separator"
        );
        assert!(lines[1].starts_with("  /ping   "));
    }

    #[test]
    fn a_heading_after_content_is_preceded_by_a_blank_line() {
        let mut list = TwoColumnList::default();
        list.push_two_columns("  /ping".to_string(), "Emits a ping".to_string());
        list.push_heading("Commands");

        assert_eq!(
            list.into_string().lines().collect::<Vec<_>>(),
            vec!["  /ping   Emits a ping", "", "Commands:"]
        );
    }
}
