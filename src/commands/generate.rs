//! `generate-config`: put what is installed under management.

use anyhow::{Result, bail};
use std::path::Path;

use super::installed;
use crate::atomic;
use crate::graph::{ENTRY, Graph, Tree};
use crate::manager::{self, Manager};
use crate::packages::PackageSpec;
use crate::report;

/// What a new file begins with: how to read it, since it is the one place a
/// person starting out will look.
const HEADER: &str = "\
# What this machine declares. vow installs what each list names and removes
# what it does not, for every manager listed here; a manager left out is left
# alone, and `manager = []` keeps it at nothing installed.
#
# A package is `name`, or `name version` to hold an exact version. A link
# keeps a file in this folder and puts a symlink to it where it belongs:
#
#   [links]
#   nvim = { source = \"./nvim\", target = \"~/.config/nvim\" }
#
# `includes` reads other files as if they were part of this one, so machines
# can share what they have in common. Paths are relative to this file.

includes = []
";

/// Write a file declaring everything installed for each installed manager,
/// or only those named. An existing file is never touched: folding today's
/// installed set into a declaration would undo what `switch` prunes.
pub fn generate_config(
    path: Option<&Path>,
    file: Option<&Path>,
    requested: &[String],
    pin: bool,
) -> Result<()> {
    let graph = Graph::discover(path)?;
    let target = graph.file(file.unwrap_or(Path::new(ENTRY)))?;
    if target.exists() {
        bail!(
            "{} already exists, and vow never rewrites a file you keep; edit it, or \
             pass `--file` to write another",
            report::path(&target)
        );
    }

    let managers = installed(requested)?;
    let mut text = String::from(HEADER);
    text.push_str("\n[packages]\n");
    for manager in &managers {
        let specs = packages_of(*manager, pin)?;
        text.push_str(&list(manager.id(), &specs));
        println!("{} {} package(s)", report::bold(manager.id()), specs.len());
    }
    if managers.is_empty() {
        println!("no supported package manager is installed here");
    }
    text.push_str("\n[links]\n");
    atomic::write(&target, &text)?;
    println!("{} {}", report::dim("wrote"), report::path(&target));

    // A host file applies before `config.toml`; say so if it would not be read.
    let applied = graph.entry(None, None)?;
    let tree = Tree::load(&graph, &applied);
    let written = std::fs::canonicalize(&target).unwrap_or(target.clone());
    if !tree.files.iter().any(|read| read.path == written) {
        report::note(format!(
            "this machine applies {}, which does not include it yet",
            report::path(&applied)
        ));
    }
    Ok(())
}

/// What `manager` has installed, with versions only where asked for and the
/// manager can hold one: an unpinned list keeps tracking what is current.
fn packages_of(manager: &dyn Manager, pin: bool) -> Result<Vec<PackageSpec>> {
    Ok(manager::installed(manager)?
        .into_values()
        .map(|spec| match spec.version {
            Some(version) if pin && manager.supports_pinning() => {
                PackageSpec::pinned(spec.name, version)
            }
            _ => PackageSpec::new(spec.name),
        })
        .collect())
}

/// `id = [...]`, one package to a line once there are any.
fn list(id: &str, specs: &[PackageSpec]) -> String {
    let key = toml::Value::String(id.to_string());
    let key = if id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        id.to_string()
    } else {
        key.to_string()
    };
    if specs.is_empty() {
        return format!("{key} = []\n");
    }
    let mut text = format!("{key} = [\n");
    for spec in specs {
        text.push_str(&format!("  {},\n", toml::Value::String(spec.to_string())));
    }
    text.push_str("]\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    #[test]
    fn a_written_list_reads_back_as_written() {
        let specs = [
            PackageSpec::new("@scope/pkg"),
            PackageSpec::pinned("bat", "0.24.0"),
        ];
        let text = format!("[packages]\n{}{}", list("npm", &specs), list("cargo", &[]));
        let dir = TempDir::new("generate");
        let tree = crate::testing::tree(&dir, &[("config.toml", &text)]);
        let resolved = crate::packages::resolve(&tree, "npm").expect("reads back");
        assert_eq!(resolved.declared.into_values().collect::<Vec<_>>(), {
            let mut sorted = specs.to_vec();
            sorted.sort();
            sorted
        });
        assert!(crate::packages::is_managed(&tree, "cargo"));
    }
}
