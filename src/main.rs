//! vow: declare the packages and dotfiles a machine should have, and converge to them.

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser};
use std::process::ExitCode;

use crate::cli::{Cli, Command};
use crate::commands::Ctx;

mod atomic;
mod cli;
mod commands;
mod diagnostic;
mod exec;
mod graph;
mod links;
mod manager;
mod packages;
mod report;
mod state;
#[cfg(test)]
mod testing;

/// Rust ignores `SIGPIPE`, so `vow status | head` would panic once `head`
/// exits; the default lets the process end quietly instead.
#[cfg(unix)]
fn restore_sigpipe() {
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

/// Non-zero means vow failed, never that it found drift: drift is an answer.
fn main() -> ExitCode {
    restore_sigpipe();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report::fail(&error);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let Some(command) = &cli.command else {
        Cli::command().print_help()?;
        return Ok(());
    };

    let managers = cli.managers();
    let target = &cli.target;
    let name = command.name();
    if !managers.is_empty() && !command.takes_managers() {
        bail!(
            "`{name}` takes no package manager; drop `{}`",
            managers.join(",")
        );
    }
    // Naming managers is about their packages: links are left alone.
    let mut scope = target.scope();
    if !managers.is_empty() {
        if target.links_only {
            bail!("`--links` acts on links, which no package manager has; drop one");
        }
        scope.links = false;
    }
    let path = target.path.as_deref();
    let file = target.file.as_deref();

    // These read no declarations, so a broken file does not stop them.
    match command {
        Command::Search { query } => {
            scope.packages_only("search")?;
            return commands::search(&managers, query);
        }
        Command::GenerateConfig { pin } => {
            scope.packages_only("generate-config")?;
            if target.host.is_some() {
                bail!("`generate-config` writes `config.toml` or `--file`; drop `--host`");
            }
            return commands::generate_config(path, file, &managers, *pin);
        }
        _ => {}
    }

    let ctx = Ctx::new(path, file, target.host.as_deref())?;
    match command {
        Command::Status => commands::status(&ctx, &managers, scope),
        Command::Switch(opts) => commands::switch(&ctx, &managers, opts, scope),
        Command::Outdated => {
            scope.packages_only("outdated")?;
            commands::outdated(&ctx, &managers)
        }
        Command::Links => {
            scope.links_only("links")?;
            commands::links(&ctx)
        }
        Command::Managed => commands::managed(&ctx, scope),
        Command::Search { .. } | Command::GenerateConfig { .. } => unreachable!("handled above"),
    }
}
