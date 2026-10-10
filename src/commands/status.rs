//! `status`: how this machine differs from what it declares.

use anyhow::{Result, bail};

use super::{Ctx, Scope, warn};
use crate::links::{LinkPlan, Links, Record};
use crate::packages::{self, Convergence};
use crate::report;

/// What `switch` would change: packages, then links. A half with nothing to
/// change prints nothing.
pub fn status(ctx: &Ctx, requested: &[String], scope: Scope) -> Result<()> {
    let links = scope.links.then(|| Links::load(&ctx.tree));
    warn(ctx, requested, scope, links.as_ref())?;

    let mut changes: Vec<Convergence> = Vec::new();
    let mut unreadable = 0;
    let mut managed = false;
    if scope.packages {
        let (present, waiting) = ctx.managers(requested)?;
        managed = !present.is_empty() || !waiting.is_empty();
        let planned = packages::plan_all(&ctx.tree, &present)
            .into_iter()
            .map(|(manager, plan)| (manager, plan.map(|plan| plan.changes)));
        // Not installed yet: nothing to compare, but its declarations are still checked.
        let checked = waiting.iter().map(|&manager| {
            let checked = packages::declared(&ctx.tree, manager);
            (manager, checked.map(|_| Convergence::default()))
        });
        for (manager, result) in planned.chain(checked) {
            match result {
                Ok(change) => changes.push(change),
                Err(error) => {
                    report::fail_under(manager.id(), &error);
                    unreadable += 1;
                }
            }
        }
    }

    for change in &changes {
        report::emit(&change.warnings());
    }

    let link_plan = links
        .as_ref()
        .filter(|links| links.check().is_ok())
        .map(|links| LinkPlan::new(links, &Record::load()));

    let show_packages = changes.iter().any(Convergence::has_work);
    let show_links = link_plan.as_ref().is_some_and(LinkPlan::has_work);

    if show_packages {
        if show_links {
            println!("{}", report::bold("packages"));
        }
        for changes in &changes {
            report::plan(changes);
        }
        println!("{}", report::summarize(&changes));
    }

    // Checked after the package half, so a fault in a link never hides it.
    if let Some(links) = &links {
        links.check()?;
    }

    if let Some(link_plan) = link_plan.filter(|_| show_links) {
        if show_packages {
            println!();
        }
        report::links(&link_plan);
    }

    // An unreadable manager is unknown, not matching: never "nothing to change".
    if unreadable > 0 {
        bail!("{unreadable} manager(s) could not be read; nothing above includes them");
    }
    if !show_packages && !show_links {
        let declared = managed || links.as_ref().is_some_and(Links::is_declared);
        if declared {
            println!("{}", report::dim("nothing to change"));
        } else {
            println!("{}", ctx.nothing_declared(scope));
        }
    }
    Ok(())
}
