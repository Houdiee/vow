//! `outdated` and `search`: questions put to each manager's own index.

use anyhow::{Result, bail};
use std::collections::BTreeMap;

use super::{Ctx, Scope, installed};
use crate::packages::{self, PackageSpec};
use crate::report::{self, item};

/// Declared packages with a newer version available. Not a fault: an unpinned
/// entry means "any version", so this is for deciding whether to bump a pin.
pub fn outdated(ctx: &Ctx, requested: &[String]) -> Result<()> {
    let (managers, waiting) = ctx.managers(requested)?;
    if managers.is_empty() && waiting.is_empty() {
        println!(
            "{}",
            ctx.nothing_declared(Scope {
                packages: true,
                links: false
            })
        );
        return Ok(());
    }

    let mut asked = 0;
    let mut pinned_behind = 0;
    let mut failed = 0;

    for manager in managers {
        let Some(command) = manager.outdated_command() else {
            continue;
        };
        asked += 1;

        // Most exit non-zero when something is outdated; only failing to run is a failure.
        let output = match command.capture_any_status() {
            Ok(output) => output,
            Err(error) => {
                report::fail_under(manager.id(), &error);
                failed += 1;
                continue;
            }
        };
        let available = manager.parse_outdated(&output);
        if available.is_empty() {
            continue;
        }
        let declared: BTreeMap<String, PackageSpec> = packages::declared(&ctx.tree, manager)?
            .declared
            .into_values()
            .map(|spec| (manager.canonical(&spec.name), spec))
            .collect();

        let mut lines = Vec::new();
        for update in &available {
            let Some(wanted) = declared.get(&manager.canonical(&update.name)) else {
                continue;
            };
            let newer = update.version.as_deref().unwrap_or("?");
            match &wanted.version {
                Some(pinned) if Some(pinned) != update.version.as_ref() => {
                    pinned_behind += 1;
                    lines.push(format!(
                        "{} {} {} {newer}",
                        report::bold(&update.name),
                        report::dim(pinned),
                        report::dim("->"),
                    ));
                }
                // A name that selects its own version is held like a pin.
                Some(_) => {}
                None if manager.name_selects_version(&update.name) => {}
                None => lines.push(format!(
                    "{} {}",
                    update.name,
                    report::dim(&format!(
                        "-> {newer} (unpinned; an ordinary upgrade picks this up)"
                    ))
                )),
            }
        }

        if !lines.is_empty() {
            println!("{}", report::bold(manager.id()));
            for line in lines {
                item(1, line);
            }
            println!();
        }
    }

    if !waiting.is_empty() {
        let names: Vec<&str> = waiting.iter().map(|manager| manager.id()).collect();
        report::note(format!(
            "not installed here, so not asked: {}",
            names.join(", ")
        ));
    }
    if asked == 0 {
        println!(
            "{}",
            report::dim("none of those managers can report what is outdated")
        );
    } else if pinned_behind == 0 {
        println!("{}", report::dim("no pinned package is behind"));
    } else {
        println!("{pinned_behind} pinned package(s) behind.");
    }
    if failed > 0 {
        bail!("{failed} manager(s) could not be asked; nothing above includes them");
    }
    Ok(())
}

/// Each manager's own search, shown as it printed it: a search result is
/// prose. Every installed manager is asked, managed or not, since this is how
/// you choose where to install something from.
pub fn search(requested: &[String], query: &str) -> Result<()> {
    let managers = installed(requested)?;
    if managers.is_empty() {
        println!("no supported package manager is installed here");
        return Ok(());
    }

    let mut searched = 0;
    for manager in managers {
        let Some(command) = manager.search_command(query) else {
            continue;
        };

        let (text, result) = command.run_captured();
        println!("{}", report::bold(manager.id()));
        // Most exit non-zero when they find nothing, which is not a failure.
        if let Err(error) = result {
            item(1, report::dim(&format!("{error:#}")));
        }
        for line in text.lines() {
            item(1, line);
        }
        println!();
        searched += 1;
    }

    if searched == 0 {
        println!(
            "{}",
            report::dim("none of those managers has a search of its own")
        );
    }
    Ok(())
}
