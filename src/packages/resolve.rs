//! Every file's declarations for a manager, collapsed into one declared state.

use anyhow::Result;
use std::collections::BTreeMap;

use super::PackageSpec;
use super::origin::{self, Origin};
use crate::diagnostic::{Diagnostic, Diagnostics, Span};
use crate::graph::Tree;
use crate::manager;

/// The declared state for one manager, after every file is read.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub declared: BTreeMap<String, PackageSpec>,
    /// Where each declared package was written, for faults found later.
    pub origins: BTreeMap<String, Origin>,
}

/// Whether any file has a key for `manager`. One with none is left alone;
/// `manager = []` keeps it at nothing installed.
pub fn is_managed(tree: &Tree, manager: &str) -> bool {
    tree.files
        .iter()
        .any(|file| file.doc.packages.keys().any(|key| key.get_ref() == manager))
}

/// Collapse every file into one declared state, merged as Nix merges modules:
/// alike merges, differently is an error (no file outranks another), a repeat in
/// one list is a slip, and an empty list contradicts packages listed elsewhere.
pub fn resolve(tree: &Tree, manager: &str) -> Result<Resolved> {
    let mut resolved = Resolved::default();
    // The file each name was first declared in.
    let mut first: BTreeMap<String, usize> = BTreeMap::new();
    let mut problems = Diagnostics::default();
    // The files with an empty list, where it was written.
    let mut empty: Vec<(usize, Span)> = Vec::new();

    for (index, file) in tree.files.iter().enumerate() {
        let Some((key, list)) = file
            .doc
            .packages
            .iter()
            .find(|(key, _)| key.get_ref() == manager)
        else {
            continue;
        };
        if list.is_empty() {
            empty.push((index, key.span()));
            continue;
        }
        // Each name this list declares, to catch a repeat.
        let mut here: BTreeMap<String, (PackageSpec, Origin)> = BTreeMap::new();
        for item in list {
            let (spec, origin) = match origin::read(file, item) {
                Ok(read) => read,
                Err(problem) => {
                    problems.push(problem);
                    continue;
                }
            };
            if let Some((earlier, at)) = here.get(&spec.name) {
                problems.push(repeated(&spec, &origin, earlier, at));
                continue;
            }
            here.insert(spec.name.clone(), (spec.clone(), origin.clone()));

            if let Some(&earlier) = first.get(&spec.name) {
                let theirs = &resolved.declared[&spec.name];
                if *theirs != spec {
                    problems.push(
                        origin
                            .fault(
                                Diagnostic::error(format!(
                                    "`{}` is already declared as `{theirs}` in `{}`",
                                    spec.name,
                                    tree.name(&tree.files[earlier].path)
                                )),
                                "declared differently here",
                            )
                            .note(format!(
                                "first declared at {}; files merge only what they declare \
                                 alike, and none overrides another: make them agree, or \
                                 declare the package in one file",
                                resolved.origins[&spec.name].locate()
                            )),
                    );
                }
                continue;
            }
            first.insert(spec.name.clone(), index);
            resolved.origins.insert(spec.name.clone(), origin);
            resolved.declared.insert(spec.name.clone(), spec);
        }
    }

    if let Some(listed) = first.values().min() {
        let listed = tree.name(&tree.files[*listed].path);
        for (index, span) in empty {
            problems.push(
                tree.files[index]
                    .source
                    .fault(
                        Diagnostic::error(format!(
                            "`{manager}` is declared empty here, but `{listed}` lists packages for it"
                        ))
                        .at(span, "declared empty"),
                    )
                    .note(
                        "an empty list keeps a manager at nothing installed, so no other file \
                         can add to it: drop the empty list, or the packages",
                    ),
            );
        }
    }

    problems.into_result()?;
    Ok(resolved)
}

/// A package declared twice in one list, pointing at the repeat to delete.
fn repeated(spec: &PackageSpec, at: &Origin, earlier: &PackageSpec, first: &Origin) -> Diagnostic {
    let problem = if spec == earlier {
        Diagnostic::error(format!(
            "`{}` is declared twice to the same effect",
            spec.name
        ))
        .note("the second says nothing the first did not")
    } else {
        Diagnostic::error(format!(
            "`{}` is declared twice, as `{earlier}` and `{spec}`",
            spec.name
        ))
        .note("a list declares each package once: keep the one you mean")
    };
    first.also(
        at.fault(problem, "declared again here"),
        "first declared here",
    )
}

/// Probable mistakes: a `[packages]` key naming no manager, and a declared manager
/// not installed here (only those in `only`, when given).
pub fn warnings(tree: &Tree, only: &[String]) -> Diagnostics {
    let mut warnings = Diagnostics::default();
    for file in &tree.files {
        for key in file.doc.packages.keys() {
            if manager::get(key.get_ref()).is_none() {
                warnings.push(
                    file.source
                        .fault(
                            Diagnostic::warning(format!(
                                "`{}` names no package manager",
                                key.get_ref()
                            ))
                            .at(key.span(), "its packages are ignored"),
                        )
                        .note("`vow managed` lists the names that are read"),
                );
            }
        }
    }

    for manager in
        manager::all().filter(|manager| only.is_empty() || only.iter().any(|id| id == manager.id()))
    {
        let id = manager.id();
        let declared = tree.files.iter().find_map(|file| {
            file.doc
                .packages
                .keys()
                .find(|key| key.get_ref() == id)
                .map(|key| (file, key))
        });
        let Some((file, key)) = declared else {
            continue;
        };
        if !manager.is_installed() {
            warnings.push(
                file.source.fault(
                    Diagnostic::warning(format!("`{id}` is declared but not installed here"))
                        .at(key.span(), "not installed")
                        .note(
                            "a switch acts on it once another manager installs it -- list it \
                             as a package of one, as `apt` can list `cargo` -- and skips it \
                             otherwise",
                        ),
                ),
            );
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TempDir, tree};

    fn names(resolved: &Resolved) -> Vec<String> {
        resolved.declared.keys().cloned().collect()
    }

    #[test]
    fn included_files_are_a_plain_union_and_agreeing_lines_merge() {
        let dir = TempDir::new("union");
        let tree = tree(
            &dir,
            &[
                (
                    "config.toml",
                    "includes = [\"dev.toml\"]\n[packages]\ncargo = [\"ripgrep\", \"fd 10.0.0\"]\n",
                ),
                ("dev.toml", "[packages]\ncargo = [\"bat\", \"fd 10.0.0\"]\n"),
            ],
        );
        let resolved = resolve(&tree, "cargo").expect("merges");
        assert_eq!(names(&resolved), ["bat", "fd", "ripgrep"]);
        assert!(resolved.origins.contains_key("bat"));
        assert!(is_managed(&tree, "cargo") && !is_managed(&tree, "npm"));
    }

    #[test]
    fn a_file_cannot_change_another_files_declaration() {
        let dir = TempDir::new("no-override");
        let tree = tree(
            &dir,
            &[
                (
                    "config.toml",
                    "includes = [\"dev.toml\"]\n[packages]\ncargo = [\"ripgrep 14.0.0\", \"bat\"]\n",
                ),
                (
                    "dev.toml",
                    "[packages]\ncargo = [\"ripgrep 14.2.0\", \"bat 0.24.0\"]\n",
                ),
            ],
        );
        let shown = format!("{}", resolve(&tree, "cargo").expect_err("refused"));
        assert!(
            shown.contains("`ripgrep` is already declared as `ripgrep 14.0.0` in `config.toml`"),
            "{shown}"
        );
        assert!(
            shown.contains("`bat` is already declared as `bat`"),
            "{shown}"
        );
        assert!(shown.contains("dev.toml:2:"), "located: {shown}");
    }

    #[test]
    fn a_package_repeated_in_one_list_or_malformed_is_an_error() {
        let dir = TempDir::new("repeat");
        let tree = tree(
            &dir,
            &[(
                "config.toml",
                "[packages]\ncargo = [\n  \"ripgrep 14.0.0\",\n  \"ripgrep 14.1.0\",\n  \"bat\",\n  \"bat\",\n  \"fd 1 2\",\n]\n",
            )],
        );
        let shown = resolve(&tree, "cargo").expect_err("repeats").to_string();
        assert!(
            shown.contains("`ripgrep` is declared twice, as `ripgrep 14.0.0` and `ripgrep 14.1.0`"),
            "{shown}"
        );
        assert!(
            shown.contains("`bat` is declared twice to the same effect"),
            "{shown}"
        );
        assert!(shown.contains("config.toml:7:"), "the third field: {shown}");
    }

    #[test]
    fn an_empty_list_merges_only_with_files_that_list_nothing_for_it() {
        let dir = TempDir::new("empty-list");
        let tree = tree(
            &dir,
            &[
                (
                    "config.toml",
                    "includes = [\"a.toml\", \"b.toml\"]\n[packages]\npnpm = []\ncargo = []\n",
                ),
                ("a.toml", "[packages]\npnpm = [\"is-odd\"]\ncargo = []\n"),
                ("b.toml", "[packages]\nnpm = [\"pnpm\"]\n"),
            ],
        );
        let shown = resolve(&tree, "pnpm").expect_err("contradicts").to_string();
        assert!(
            shown.contains("`pnpm` is declared empty here, but `a.toml` lists packages for it"),
            "{shown}"
        );
        assert!(shown.contains("config.toml:3:1"), "{shown}");
        assert!(resolve(&tree, "cargo").expect("agree").declared.is_empty());
        assert_eq!(names(&resolve(&tree, "npm").expect("alone")), ["pnpm"]);
    }
}
