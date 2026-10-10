//! `switch`: the one command that changes the machine.

use anyhow::{Context, Result, anyhow};

use super::{Ctx, Managers, Scope, warn};
use crate::cli::SwitchOpts;
use crate::diagnostic::Diagnostic;
use crate::exec::{self, Invocation};
use crate::links::{LinkPlan, Links, Record};
use crate::manager::Manager;
use crate::packages::{self, Convergence, Plan};
use crate::report::{self, item};

/// Converge this machine: place links, then install and remove packages, a
/// stage at a time (see [`Stage`]).
///
/// Everything is read and checked before anything changes, the plan is shown,
/// and one answer covers both halves. A half with nothing to change prints
/// nothing. A declared manager that another installs is planned, shown and
/// asked about once it is there.
pub fn switch(ctx: &Ctx, requested: &[String], opts: &SwitchOpts, scope: Scope) -> Result<()> {
    let links = scope.links.then(|| Links::load(&ctx.tree));
    warn(ctx, requested, scope, links.as_ref())?;

    let record = Record::load();
    let link_plan = if let Some(links) = &links {
        links.check()?;
        let link_plan = LinkPlan::new(links, &record);
        link_plan.check()?;
        Some(link_plan)
    } else {
        None
    };

    let (plans, waiting) = if scope.packages {
        let (present, waiting) = ctx.managers(requested)?;
        // Checked now, not once a stage installs it and the switch is half done.
        for manager in &waiting {
            packages::declared(&ctx.tree, *manager).with_context(|| {
                format!(
                    "refusing to apply: `{}` declares what it cannot hold",
                    manager.id()
                )
            })?;
        }
        let plans = read_packages(ctx, &present)?;
        for plan in &plans {
            report::emit(&plan.changes.warnings());
        }
        (plans, waiting)
    } else {
        (Vec::new(), Vec::new())
    };

    let show_packages = has_work(&plans, opts.upgrade);
    let show_links = link_plan.as_ref().is_some_and(LinkPlan::has_work);

    if show_packages {
        if show_links {
            println!("{}", report::bold("packages"));
        }
        print_packages(&plans, opts.upgrade);
    }
    if let Some(link_plan) = link_plan.as_ref().filter(|_| show_links) {
        if show_packages {
            println!();
        }
        report::links(link_plan);
    }

    if !show_packages && !show_links {
        // Record what is in place even with nothing to change, so a lost record heals;
        // a dry run changes nothing at all.
        if let Some(link_plan) = link_plan.as_ref().filter(|_| !opts.dry_run) {
            Record::save(&link_plan.kept())?;
        }
        let declared = !plans.is_empty()
            || !waiting.is_empty()
            || links.as_ref().is_some_and(Links::is_declared);
        if declared {
            println!("{}", report::dim("nothing to change"));
        } else {
            println!("{}", ctx.nothing_declared(scope));
        }
        return Ok(());
    }
    if opts.dry_run {
        println!("{}", report::dim("dry run: nothing changed"));
        return Ok(());
    }
    if !opts.yes && !report::confirm("Go ahead?")? {
        println!("{}", report::dim("aborted"));
        return Ok(());
    }

    // Links first, so a manager that writes into a linked folder (`ya pkg add`)
    // writes through the link rather than making a directory in its way.
    let mut pending = plans;
    pending.sort_by_key(|plan| (plan.manager.stage(), plan.manager.id()));
    let first: &[&str] = if show_links { &[LINKS] } else { &[] };
    let mut run = exec::Run::start(first, &queue(&pending, opts.upgrade))?;
    // The halves are independent: a failed link stops no manager, and a failed
    // manager undoes no link. Both are said at the end.
    let links_failed = match &link_plan {
        Some(link_plan) => place_links(&mut run, link_plan, &record, show_links).err(),
        None => None,
    };
    let ran = converge(ctx, opts, &mut run, pending, waiting);
    // The boxes stop before anything else is printed under them.
    drop(run);
    let packages = ran.and_then(|ran| report_what_did_not_take(ctx, &ran));
    match (links_failed, packages) {
        (None, packages) => packages,
        (Some(links_failed), Ok(())) => Err(links_failed),
        (Some(links_failed), Err(packages)) => {
            report::fail(&links_failed);
            Err(packages)
        }
    }
}

/// The box links are placed in, as a manager's are.
const LINKS: &str = "links";

/// Place the links, as their own step: a line for each, and a failure
/// reported like a manager's. What is in place is recorded even if some link
/// failed, or vow would take its own links for someone else's next time.
/// Links with nothing to change are still recorded, without a box.
fn place_links(run: &mut exec::Run, plan: &LinkPlan, record: &Record, shown: bool) -> Result<()> {
    let mut placed = Vec::new();
    let mut place = |say: &dyn Fn(&str)| {
        let applied = plan.apply(record, say);
        placed = applied.placed;
        match applied.failed {
            0 => Ok(()),
            failed => Err(anyhow!(
                "could not place {failed} of {} link(s)",
                applied.tried
            )),
        }
    };
    let outcome = if shown {
        run.task(LINKS, &mut place)
    } else {
        place(&|_| {})
    };
    Record::save(&placed)?;
    outcome
}

/// After a step failed, `why`: mark every box still waiting as not run, and
/// say which managers were left.
fn stop(run: &mut exec::Run, boxed: &[(&str, Vec<Invocation>)], waiting: &Managers, why: &str) {
    let left: Vec<&str> = boxed.iter().map(|(id, _)| *id).collect();
    run.not_run(&left);
    run.pause();
    let left: Vec<&str> = left
        .into_iter()
        .chain(waiting.iter().map(|manager| manager.id()))
        .collect();
    if !left.is_empty() {
        report::note(format!("not run, since {why}: {}", left.join(", ")));
    }
}

/// Run the plans a stage at a time, each finishing before the next starts,
/// under one set of boxes ordered by stage and then by name: a manager whose
/// stage has not begun waits in its box. After each stage, a waiting manager
/// that is now installed is planned and, unless `--yes`, asked about: nobody
/// has seen its plan yet. A stage that fails stops the rest, which may rely on
/// it. Returns the managers that ran.
fn converge(
    ctx: &Ctx,
    opts: &SwitchOpts,
    run: &mut exec::Run,
    mut pending: Vec<Plan>,
    mut waiting: Managers,
) -> Result<Managers> {
    let order = |plans: &mut Vec<Plan>| {
        plans.sort_by_key(|plan| (plan.manager.stage(), plan.manager.id()));
    };
    let mut ran: Managers = Vec::new();

    while let Some(stage) = pending.iter().map(|plan| plan.manager.stage()).min() {
        let (now, later): (Vec<Plan>, Vec<Plan>) = pending
            .into_iter()
            .partition(|plan| plan.manager.stage() == stage);
        pending = later;

        let queued = queue(&now, opts.upgrade);
        if !queued.is_empty() {
            if let Err(error) = run.stage(&queued) {
                let why = "an earlier stage failed";
                stop(run, &queue(&pending, opts.upgrade), &waiting, why);
                return Err(error);
            }
            ran.extend(
                now.iter()
                    .map(|plan| plan.manager)
                    .filter(|manager| queued.iter().any(|(id, _)| *id == manager.id())),
            );
        }

        let (arrived, still): (Managers, Managers) = waiting
            .into_iter()
            .partition(|manager| manager.is_installed());
        waiting = still;
        if arrived.is_empty() {
            continue;
        }
        let mut plans = read_packages(ctx, &arrived)?;
        if !has_work(&plans, opts.upgrade) {
            continue;
        }
        run.pause();
        println!();
        let names: Vec<&str> = arrived.iter().map(|manager| manager.id()).collect();
        report::note(format!(
            "now installed, so planned too: {}",
            names.join(", ")
        ));
        print_packages(&plans, opts.upgrade);
        if !opts.yes && !report::confirm("Go ahead with these too?")? {
            println!("{}", report::dim("left as they are"));
            plans.clear();
        }
        pending.extend(plans);
        order(&mut pending);
        run.resume(&queue(&pending, opts.upgrade))?;
    }
    Ok(ran)
}

/// Whether the plans change anything, an upgrade included.
fn has_work(plans: &[Plan], upgrade: bool) -> bool {
    plans
        .iter()
        .any(|plan| plan.changes.has_work() || (upgrade && !plan.upgrade_commands().is_empty()))
}

/// The package half of a plan: each manager's changes, a summary, and with
/// `--upgrade` the commands that upgrade, which no package diff can show.
fn print_packages(plans: &[Plan], upgrade: bool) {
    let changes: Vec<Convergence> = plans.iter().map(|plan| plan.changes.clone()).collect();
    for changes in &changes {
        report::plan(changes);
    }
    let summary = report::summarize(&changes);
    if !summary.is_empty() {
        println!("{summary}");
    }
    if !upgrade {
        return;
    }
    let mut upgraded = false;
    for plan in plans {
        let commands = plan.upgrade_commands();
        if commands.is_empty() {
            continue;
        }
        upgraded = true;
        println!("{}", report::bold(plan.manager.id()));
        for command in commands {
            item(1, format!("{} {}", report::dim("$"), command.display()));
        }
    }
    if upgraded {
        report_what_is_held(plans);
    }
}

/// Each manager's commands for its plan, and with `--upgrade` its upgrade.
fn queue(plans: &[Plan], upgrade: bool) -> Vec<(&'static str, Vec<Invocation>)> {
    plans
        .iter()
        .map(|plan| {
            let mut commands = plan.commands();
            if upgrade {
                commands.extend(plan.upgrade_commands());
            }
            (plan.manager.id(), commands)
        })
        .filter(|(_, commands)| !commands.is_empty())
        .collect()
}

/// Every manager's plan. Acting on a half-known system is how the wrong
/// things get uninstalled, so one that cannot be read stops the switch.
fn read_packages(ctx: &Ctx, managers: &[&'static dyn Manager]) -> Result<Vec<Plan>> {
    packages::plan_all(&ctx.tree, managers)
        .into_iter()
        .map(|(manager, result)| {
            result.with_context(|| {
                format!("refusing to apply: `{}` could not be planned", manager.id())
            })
        })
        .collect()
}

/// Name the pins an upgrade leaves alone, so a held pin is not mistaken for
/// one that was never read.
fn report_what_is_held(plans: &[Plan]) {
    let held: Vec<(&str, Vec<String>)> = plans
        .iter()
        .map(|plan| (plan.manager.id(), plan.pinned()))
        .filter(|(_, pinned)| !pinned.is_empty())
        .collect();
    if held.is_empty() {
        return;
    }

    let total: usize = held.iter().map(|(_, names)| names.len()).sum();
    println!();
    report::note(format!("{total} pinned, left alone:"));
    for (id, names) in &held {
        item(1, report::dim(&format!("{id}: {}", names.join(" "))));
    }
}

/// Re-read the machine and fail if anything should have changed but did not:
/// a command can exit zero and achieve nothing.
fn report_what_did_not_take(ctx: &Ctx, managers: &[&'static dyn Manager]) -> Result<()> {
    let mut notes: Vec<String> = Vec::new();
    let (mut missing, mut lingering) = (false, false);
    for (manager, result) in packages::plan_all(&ctx.tree, managers) {
        let changes = match result {
            Ok(plan) if plan.changes.has_work() => plan.changes,
            Ok(_) => continue,
            // Unknown is not the declared state either.
            Err(error) => {
                notes.push(format!(
                    "{}: could not be read back: {error:#}",
                    manager.id()
                ));
                continue;
            }
        };
        missing |= !changes.install.is_empty() || !changes.repin.is_empty();
        lingering |= !changes.remove.is_empty();
        let what: Vec<String> = changes
            .install
            .iter()
            .map(|spec| format!("{} not installed", spec.name))
            .chain(
                changes
                    .repin
                    .iter()
                    .map(|change| format!("{} still at {}", change.spec.name, change.installed)),
            )
            .chain(
                changes
                    .remove
                    .iter()
                    .map(|spec| format!("{} still installed", spec.name)),
            )
            .collect();
        notes.push(format!("{}: {}", changes.manager, what.join(", ")));
    }
    if notes.is_empty() {
        return Ok(());
    }

    let mut error = Diagnostic::error(format!(
        "applied, but {} manager(s) did not reach the declared state",
        notes.len()
    ));
    for note in notes {
        error = error.note(note);
    }
    if missing {
        error = error.note("check each declared name and version exists");
    }
    // A manager whose packages share one dependency tree, such as composer,
    // keeps what another package still needs.
    if lingering {
        error = error.note("check nothing still installed depends on what was to go");
    }
    Err(error.into())
}
