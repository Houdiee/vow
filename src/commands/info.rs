//! `managed` and `links`: what the graph and the machine hold.

use anyhow::Result;

use super::{Ctx, Scope};
use crate::links::{LinkPlan, Links, Record};
use crate::manager;
use crate::packages;
use crate::report::{self, item, marked};

/// What this machine manages: every package manager vow knows, with the two
/// conditions for acting on it, then each file read and what it declares.
pub fn managed(ctx: &Ctx, scope: Scope) -> Result<()> {
    if scope.packages {
        managers(ctx);
        println!();
    }
    files(ctx, scope);
    println!();
    println!(
        "{}",
        report::dim(&format!("applies: {}", report::path(&ctx.tree.entry)))
    );
    println!(
        "{}",
        report::dim(&format!("graph: {}", report::path(ctx.graph.root())))
    );
    Ok(())
}

/// Every manager vow knows, marking the ones acted on here: installed and
/// declared.
fn managers(ctx: &Ctx) {
    let rows: Vec<(bool, Vec<String>)> = manager::all()
        .map(|manager| {
            let installed = manager.is_installed();
            let declared = packages::is_managed(&ctx.tree, manager.id());
            (
                installed && declared,
                vec![
                    manager.id().to_string(),
                    yes_no(installed),
                    yes_no(declared),
                ],
            )
        })
        .collect();
    table(&["manager", "installed", "declared"], &rows);
}

/// Each file read, in the order read, with the managers and links it declares.
fn files(ctx: &Ctx, scope: Scope) {
    if !ctx.tree.exists() {
        println!("{}", ctx.nothing_declared(scope));
        return;
    }
    let mut header = vec!["file"];
    if scope.packages {
        header.push("packages");
    }
    if scope.links {
        header.push("links");
    }
    let rows: Vec<(bool, Vec<String>)> = ctx
        .tree
        .files
        .iter()
        .map(|file| {
            let mut row = vec![ctx.tree.name(&file.path)];
            if scope.packages {
                let managers: Vec<String> = file
                    .doc
                    .packages
                    .keys()
                    .map(|key| key.get_ref().clone())
                    .collect();
                row.push(list(&managers));
            }
            if scope.links {
                row.push(match file.doc.links.len() {
                    0 => "-".to_string(),
                    count => count.to_string(),
                });
            }
            (file.path == ctx.tree.entry, row)
        })
        .collect();
    table(&header, &rows);
}

fn yes_no(yes: bool) -> String {
    if yes { "yes" } else { "-" }.to_string()
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "-".to_string()
    } else {
        names.join(", ")
    }
}

/// Rows under a dim header, each column padded to its widest cell. A marked
/// row is led by `*`.
fn table(header: &[&str], rows: &[(bool, Vec<String>)]) {
    let widths: Vec<usize> = (0..header.len())
        .map(|column| {
            rows.iter()
                .map(|(_, row)| row[column].chars().count())
                .chain([header[column].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: &[&str]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        padded.join("  ").trim_end().to_string()
    };
    item(1, report::dim(&line(header)));
    for (mark, row) in rows {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        let mark = mark.then(|| report::accent("*"));
        println!("{}", marked(mark, line(&cells)));
    }
}

/// Every link this machine declares, whether it is in place, and which file
/// declares it; then the entries nothing links.
pub fn links(ctx: &Ctx) -> Result<()> {
    let links = Links::load(&ctx.tree);
    links.check()?;
    let plan = LinkPlan::new(&links, &Record::load());

    // Asked directly, so an empty answer says where links would go -- unless
    // links vow placed are still to be removed.
    if !links.is_declared() && plan.changes.is_empty() {
        println!("{}", report::dim("no links declared"));
        for file in ctx.files() {
            println!("{}", report::dim(&format!("looked in {}", file.display())));
        }
        return Ok(());
    }

    report::every_link(&plan);
    report::unlinked(&links.unlinked);
    Ok(())
}
