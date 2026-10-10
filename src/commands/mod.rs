//! One function per command. Each reads the graph, asks the managers, and says
//! what it found through `report`; only `switch` changes the machine.

mod generate;
mod info;
mod query;
mod status;
mod switch;

pub use generate::generate_config;
pub use info::{links, managed};
pub use query::{outdated, search};
pub use status::status;
pub use switch::switch;

use anyhow::{Result, anyhow, bail};
use std::path::{Path, PathBuf};

use crate::diagnostic::Diagnostic;
use crate::graph::{self, Graph, Tree};
use crate::links::Links;

pub(super) type Managers = Vec<&'static dyn Manager>;
use crate::manager::{self, Manager};
use crate::packages;
use crate::report;

/// The graph a run reads, and the file this machine applies with all it
/// includes.
pub struct Ctx {
    pub graph: Graph,
    pub tree: Tree,
    /// This machine's name, when the graph has host files and none is its.
    unmatched: Option<String>,
}

impl Ctx {
    /// Read the graph at `path`, applying `file`, else `host`'s file, else
    /// this machine's. A file that cannot be read stops everything, since
    /// nothing it declares can be trusted to be complete.
    pub fn new(path: Option<&Path>, file: Option<&Path>, host: Option<&str>) -> Result<Self> {
        let graph = Graph::discover(path)?;
        graph.check()?;
        let entry = graph.entry(file, host)?;
        let tree = Tree::load(&graph, &entry);
        tree.check()?;
        let unmatched = if file.is_none() && host.is_none() && !graph.hosts().is_empty() {
            match graph.this_host()? {
                Some(_) => None,
                None => Some(graph::hostname()?),
            }
        } else {
            None
        };
        Ok(Self {
            graph,
            tree,
            unmatched,
        })
    }

    /// The files read, as paths in the graph, in the order read.
    pub fn files(&self) -> Vec<PathBuf> {
        self.tree
            .files
            .iter()
            .map(|file| PathBuf::from(self.tree.name(&file.path)))
            .collect()
    }

    /// The managers to act on -- those named, else every one declared here --
    /// split into the installed and those not installed yet, which a switch
    /// acts on once an earlier stage installs them.
    pub(super) fn managers(&self, requested: &[String]) -> Result<(Managers, Managers)> {
        let candidates: Managers = if requested.is_empty() {
            manager::all()
                .filter(|manager| self.declares(*manager))
                .collect()
        } else {
            self.declared(requested)?
        };
        Ok(candidates
            .into_iter()
            .partition(|manager| manager.is_installed()))
    }

    /// Whether this machine's files have a list for `manager`.
    fn declares(&self, manager: &dyn Manager) -> bool {
        packages::is_managed(&self.tree, manager.id())
    }

    /// Refuse a named manager this machine does not declare: with no list,
    /// nothing says what it should hold, and planning it would remove
    /// everything it has.
    fn declared(&self, requested: &[String]) -> Result<Managers> {
        let managers = named(requested)?;
        if let Some(stray) = managers.iter().find(|manager| !self.declares(**manager)) {
            bail!(
                "`{id}` is not declared here, so vow leaves it alone; give it a list \
                 under `[packages]`, or `{id} = []` to keep it at nothing installed",
                id = stray.id()
            );
        }
        Ok(managers)
    }

    /// What to say when the halves in `scope` declare nothing.
    fn nothing_declared(&self, scope: Scope) -> String {
        if self.tree.exists() {
            let entry = self.tree.name(&self.tree.entry);
            match (scope.packages, scope.links) {
                (false, _) => format!("no links declared in `{entry}`"),
                (_, false) => format!("no packages declared in `{entry}`"),
                _ => format!("`{entry}` declares nothing yet"),
            }
        } else {
            format!(
                "nothing declared yet: there is no {}; `vow generate-config` writes one \
                 from what is installed",
                report::path(&self.tree.entry)
            )
        }
    }
}

/// Say what is probably a mistake in the halves in scope, in one block before
/// the report so it never interleaves with it.
fn warn(ctx: &Ctx, requested: &[String], scope: Scope, links: Option<&Links>) -> Result<()> {
    let mut warnings = ctx.graph.warnings(&ctx.tree);
    warnings.extend(ctx.tree.warnings(&ctx.graph));
    if let Some(name) = &ctx.unmatched {
        warnings.push(
            Diagnostic::warning(format!(
                "this machine, `{name}`, has no host file, so `{}` applies",
                ctx.tree.name(&ctx.tree.entry)
            ))
            .note(format!(
                "the hosts are {}; set VOW_HOST to name this machine",
                ctx.graph.hosts().join(", ")
            )),
        );
    }
    if scope.packages {
        warnings.extend(packages::warnings(&ctx.tree, requested));
    }
    if let Some(links) = links {
        warnings.extend(links.warnings().clone());
    }
    report::emit(&warnings);
    Ok(())
}

/// Which halves of the graph a command speaks about.
#[derive(Debug, Clone, Copy)]
pub struct Scope {
    pub packages: bool,
    pub links: bool,
}

impl Scope {
    /// Refuse `--links` for a command that only speaks about packages.
    pub fn packages_only(self, command: &str) -> Result<()> {
        if !self.packages {
            bail!("`{command}` is about packages only; drop `--links`");
        }
        Ok(())
    }

    /// Refuse `--packages` for a command that only speaks about links.
    pub fn links_only(self, command: &str) -> Result<()> {
        if !self.links {
            bail!("`{command}` is about links only; drop `--packages`");
        }
        Ok(())
    }
}

/// The managers named, each of which must be installed here, or when none are
/// named, every installed one: what `generate-config` and `search` ask.
fn installed(requested: &[String]) -> Result<Managers> {
    if requested.is_empty() {
        return Ok(manager::all()
            .filter(|manager| manager.is_installed())
            .collect());
    }
    let managers = named(requested)?;
    if let Some(missing) = managers.iter().find(|manager| !manager.is_installed()) {
        bail!("`{}` is not installed on this machine", missing.id());
    }
    Ok(managers)
}

/// The managers named, each once, in registry order, installed or not.
fn named(requested: &[String]) -> Result<Managers> {
    let mut managers = requested
        .iter()
        .map(|id| {
            manager::get(id)
                .ok_or_else(|| anyhow!("unknown package manager `{id}` (see `vow managed`)"))
        })
        .collect::<Result<Vec<_>>>()?;
    managers.sort_by_key(|manager| manager.id());
    managers.dedup_by_key(|manager| manager.id());
    Ok(managers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(managers: &[&dyn Manager]) -> Vec<&'static str> {
        managers.iter().map(|manager| manager.id()).collect()
    }

    #[test]
    fn a_manager_named_twice_is_acted_on_once() {
        let requested = ["npm", "cargo", "npm"].map(String::from);
        assert_eq!(
            ids(&named(&requested).expect("known")),
            vec!["cargo", "npm"]
        );
    }
}
