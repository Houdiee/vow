//! The command line: `vow [MANAGERS] <command>`

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::commands::Scope;

#[derive(Parser)]
#[command(
    name = "vow",
    version,
    about = "Declare the packages and links your machine should have, and converge to it"
)]
pub struct Cli {
    /// Package managers to act on, comma-separated. Defaults to every managed one, or every installed one for `generate-config` and `search`
    #[arg(value_name = "MANAGERS")]
    managers: Option<String>,

    #[command(flatten)]
    pub target: Target,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    pub fn managers(&self) -> Vec<String> {
        self.managers
            .iter()
            .flat_map(|spec| spec.split(','))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Show how a machine differs from what it declares
    Status,

    /// Converge this machine on what it declares: the only command that changes it
    Switch(SwitchOpts),

    /// Write a config.toml (or `--file`) declaring everything installed; never overwrites
    #[command(name = "generate-config")]
    GenerateConfig {
        /// Hold each package at the version installed now, where its manager can
        #[arg(long)]
        pin: bool,
    },

    /// Show declared packages with a newer version available
    Outdated,

    /// Search each manager's own index and show what it says
    Search {
        /// What to look for
        #[arg(value_name = "QUERY")]
        query: String,
    },

    /// List the package managers vow supports and which this machine manages, then each file read
    Managed,

    /// Show every link this machine applies, which file declares it, and whether it is in place
    Links,
}

impl Command {
    /// The name it is typed as, for messages.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Status => "status",
            Command::Switch(_) => "switch",
            Command::GenerateConfig { .. } => "generate-config",
            Command::Outdated => "outdated",
            Command::Search { .. } => "search",
            Command::Managed => "managed",
            Command::Links => "links",
        }
    }

    /// Whether it acts on package managers, and so can be given some.
    pub fn takes_managers(&self) -> bool {
        !matches!(self, Command::Managed | Command::Links)
    }
}

/// What a switch was asked to do beyond the plain diff.
#[derive(Args, Debug, Clone, Default)]
pub struct SwitchOpts {
    /// Also bring unpinned packages up to date
    #[arg(long)]
    pub upgrade: bool,

    /// Show the plan and exit without changing anything
    #[arg(long)]
    pub dry_run: bool,

    /// Do not ask for confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// What a command acts on: the graph, the file in it, and which half.
#[derive(Args, Default)]
pub struct Target {
    /// Use a graph somewhere other than the default; overrides `$VOW_CONFIG_DIR`
    #[arg(long, global = true, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Apply another host's file, `hosts/NAME.toml` or `hosts/NAME/NAME.toml`
    #[arg(long, global = true, value_name = "NAME", conflicts_with = "file")]
    pub host: Option<String>,

    /// Apply this file, relative to the graph, instead of this machine's
    #[arg(long, global = true, value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// Act on the links only, not the packages
    #[arg(long = "links", global = true, conflicts_with = "packages_only")]
    pub links_only: bool,

    /// Act on the packages only, not the links
    #[arg(long = "packages", global = true)]
    pub packages_only: bool,
}

impl Target {
    /// Which halves to speak about; neither flag means both.
    pub fn scope(&self) -> Scope {
        Scope {
            packages: self.packages_only || !self.links_only,
            links: self.links_only || !self.packages_only,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager;
    use clap::CommandFactory;

    fn parse(argv: &[&str]) -> Cli {
        Cli::try_parse_from(argv).expect("parses")
    }

    #[test]
    fn the_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_manager_name_collides_with_a_command_name() {
        // A leading token is a manager list unless it names a command.
        let commands: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();
        for manager in manager::all() {
            assert!(
                !commands.iter().any(|name| name == manager.id()),
                "`{}` is both a manager and a command",
                manager.id()
            );
        }
    }

    #[test]
    fn managers_lead_comma_separated() {
        let cli = parse(&["vow", "cargo,npm", "status"]);
        assert_eq!(cli.managers(), vec!["cargo", "npm"]);
    }

    #[test]
    fn a_command_name_is_not_read_as_a_manager() {
        let cli = parse(&["vow", "status"]);
        assert!(cli.managers().is_empty());
        assert!(matches!(cli.command, Some(Command::Status)));
    }
}
