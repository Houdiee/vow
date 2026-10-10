//! The links half: files kept in the graph, linked to where they belong.
//! The content stays in the graph and the link points at it.

mod place;
mod record;

#[cfg(test)]
pub use place::Obstacle;
pub use place::{Action, LinkChange, LinkPlan};
pub use record::Record;

use anyhow::Result;
use std::path::{Component, Path, PathBuf};
use toml::Spanned;

use crate::diagnostic::{Diagnostic, Diagnostics, Source, Span};
use crate::graph::{File, LinkDecl, Tree, entries_in, resolve_inside};
use crate::report;

/// One link, resolved against the file that declared it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub source: PathBuf,
    /// `source` as a path in the graph; with the target, the link's identity.
    pub entry: PathBuf,
    pub target: PathBuf,
    /// The file that asked for it, as a path in the graph.
    pub file: PathBuf,
}

/// Every link one machine declares, read from every file at once.
#[derive(Debug, Default)]
pub struct Links {
    /// Every symlink asked for, in target order.
    pub placements: Vec<Placement>,
    /// Entries beside a linked one that no link places.
    pub unlinked: Vec<String>,
    /// What stops any of it from being placed.
    problems: Diagnostics,
    /// What is probably a mistake, but stops nothing.
    warnings: Diagnostics,
}

/// One link as read, with its file, so a fault can point at both.
struct Read {
    name: Spanned<String>,
    source: Source,
    placement: Placement,
    target_at: Span,
}

impl Links {
    /// Read the tree's links. Files never override each other: a link declared
    /// alike twice is one link, and anything else on one target is refused.
    pub fn load(tree: &Tree) -> Self {
        let mut links = Self::default();
        let mut read: Vec<Read> = Vec::new();
        for file in &tree.files {
            for (name, decl) in &file.doc.links {
                match links.read(tree, file, name, decl) {
                    Some(link) => read.push(link),
                    None => continue,
                }
            }
        }

        let placed = links.check_overlap(read);
        links.check_targets(tree, &placed);
        links.placements = placed.into_iter().map(|read| read.placement).collect();
        links
            .placements
            .sort_by(|left, right| left.target.cmp(&right.target));
        links.find_unlinked(tree);
        links
    }

    /// Refuse links that cannot be placed safely.
    pub fn check(&self) -> Result<()> {
        self.problems.clone().into_result()
    }

    /// What is probably a mistake, such as an entry no link places.
    pub fn warnings(&self) -> &Diagnostics {
        &self.warnings
    }

    /// Whether this machine declares any link at all.
    pub fn is_declared(&self) -> bool {
        !self.placements.is_empty() || !self.problems.is_empty()
    }

    /// One link of `file`, noting what is wrong with it.
    fn read(
        &mut self,
        tree: &Tree,
        file: &File,
        name: &Spanned<String>,
        decl: &LinkDecl,
    ) -> Option<Read> {
        let fault = |problem: Diagnostic, span: Span, label: &str| {
            file.source.fault(problem.at(span, label.to_string()))
        };
        let source = match resolve_inside(&tree.graph, file.dir(), decl.source.get_ref()) {
            Ok(source) => source,
            Err(problem) => {
                self.problems
                    .push(fault(problem, decl.source.span(), "the file to link"));
                return None;
            }
        };
        if source == tree.graph.root() || !source.exists() {
            let what = if source.exists() {
                "the graph itself cannot be linked".to_string()
            } else {
                format!("there is nothing at `{}`", tree.name(&source))
            };
            self.problems.push(
                fault(
                    Diagnostic::error(format!("`{}` links nothing", name.get_ref())),
                    decl.source.span(),
                    &what,
                )
                .note("`source` is a file or directory, relative to the file naming it"),
            );
            return None;
        }
        let target = match checked_target(decl.target.get_ref()) {
            Ok(target) => target,
            Err(problem) => {
                self.problems
                    .push(fault(problem, decl.target.span(), "where the link goes"));
                return None;
            }
        };
        Some(Read {
            name: name.clone(),
            source: file.source.clone(),
            placement: Placement {
                entry: source
                    .strip_prefix(tree.graph.root())
                    .unwrap_or(&source)
                    .to_path_buf(),
                source,
                target,
                file: PathBuf::from(tree.name(&file.path)),
            },
            target_at: decl.target.span(),
        })
    }

    /// Two links onto one target, or one name declared differently: the first keeps
    /// it, so the fault is said once.
    fn check_overlap(&mut self, candidates: Vec<Read>) -> Vec<Read> {
        let mut placed: Vec<Read> = Vec::new();
        for read in candidates {
            let same_name = placed
                .iter()
                .find(|theirs| theirs.name.get_ref() == read.name.get_ref());
            let same_target = placed
                .iter()
                .find(|theirs| theirs.placement.target == read.placement.target);
            let alike = |theirs: &Read| {
                theirs.placement.entry == read.placement.entry
                    && theirs.placement.target == read.placement.target
            };
            let problem = match (same_name, same_target) {
                (Some(theirs), _) if alike(theirs) => continue,
                (Some(theirs), _) => (
                    theirs,
                    Diagnostic::error(format!(
                        "`{}` is already declared differently in `{}`",
                        read.name.get_ref(),
                        theirs.placement.file.display()
                    ))
                    .at(read.name.span(), "declared again here"),
                ),
                (None, Some(theirs)) => (
                    theirs,
                    Diagnostic::error(format!(
                        "`{}` is already linked by `{}` in `{}`",
                        report::path(&read.placement.target),
                        theirs.name.get_ref(),
                        theirs.placement.file.display()
                    ))
                    .at(read.target_at.clone(), "linked again here"),
                ),
                (None, None) => {
                    placed.push(read);
                    continue;
                }
            };
            let (theirs, problem) = problem;
            self.problems.push(read.source.fault(problem.note(format!(
                "first declared at {}; files never override each other: declare the link \
                     once, in a file every machine that wants it includes",
                theirs.source.locate(&theirs.name.span())
            ))));
        }
        placed
    }

    /// Targets that cannot be placed safely, whichever file declared them.
    fn check_targets(&mut self, tree: &Tree, placed: &[Read]) {
        for read in placed {
            let target = &read.placement.target;
            let root = tree.graph.root();
            let problem = if target.starts_with(root) {
                Diagnostic::error(format!("`{}` is inside the graph", report::path(target)))
                    .at(read.target_at.clone(), "the graph would link to itself")
            } else if root.starts_with(target) {
                Diagnostic::error(format!("`{}` holds the graph", report::path(target)))
                    .at(read.target_at.clone(), "a link here would replace it")
            } else if let Some(outer) = placed.iter().find(|outer| {
                outer.placement.target != *target && target.starts_with(&outer.placement.target)
            }) {
                Diagnostic::error(format!(
                    "`{}` is inside `{}`, which is linked too",
                    report::path(target),
                    report::path(&outer.placement.target)
                ))
                .at(read.target_at.clone(), "inside another link")
                .note(format!(
                    "{} links the outer one into the graph, so this link would be written \
                     there; link the outer source whole, or only its parts",
                    outer.source.locate(&outer.target_at)
                ))
            } else {
                continue;
            };
            self.problems.push(read.source.fault(problem));
        }
    }

    /// Entries beside a linked one that nothing links, except in the graph's own
    /// folder or one holding a file vow reads, which hold the rest of the graph.
    fn find_unlinked(&mut self, tree: &Tree) {
        let mut folders: Vec<PathBuf> = Vec::new();
        for link in &self.placements {
            let Some(folder) = link.source.parent() else {
                continue;
            };
            let holds_vow = tree.graph.root() == folder
                || tree.files.iter().any(|file| file.path.starts_with(folder));
            if !holds_vow && !folders.iter().any(|seen| seen == folder) {
                folders.push(folder.to_path_buf());
            }
        }

        // What another machine links counts as linked: its file is kept for it.
        let elsewhere = if folders.is_empty() {
            Vec::new()
        } else {
            tree.graph.sources()
        };
        for folder in folders {
            for name in entries_in(&folder) {
                let path = folder.join(&name);
                let linked = self
                    .placements
                    .iter()
                    .map(|link| &link.source)
                    .chain(&elsewhere);
                if linked.into_iter().any(|source| source.starts_with(&path)) {
                    continue;
                }
                let entry = tree.name(&path);
                self.warnings.push(
                    Diagnostic::warning(format!("`{entry}` is kept here, but nothing links it"))
                        .in_place(&path)
                        .note("give it a link, or move it out of this folder"),
                );
                self.unlinked.push(entry);
            }
        }
    }
}

/// A plain absolute target: `..` would give one place two spellings, and a
/// control character would split the record's fields.
fn checked_target(target: &str) -> Result<PathBuf, Diagnostic> {
    if target.contains(char::is_control) {
        return Err(
            Diagnostic::error("a target cannot contain a control character")
                .note("write the path without tabs or other control characters"),
        );
    }
    // Rebuilt from its parts, so a trailing `/` names the same place.
    let path: PathBuf = Path::new(&expand_tilde(target)).components().collect();
    if !path.is_absolute() {
        return Err(
            Diagnostic::error(format!("`{target}` is not an absolute path"))
                .note("start a target with `/` or `~`"),
        );
    }
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(
            Diagnostic::error(format!("`{target}` goes back up with `..`"))
                .note("write the path out plainly"),
        );
    }
    Ok(path)
}

/// Expand a leading `~` to the home directory.
fn expand_tilde(target: &str) -> String {
    let Some(rest) = target.strip_prefix('~') else {
        return target.to_string();
    };
    // `~other` is another user's home, which is left as written.
    if !rest.is_empty() && !rest.starts_with('/') {
        return target.to_string();
    }
    match dirs::home_dir() {
        Some(home) => format!("{}{rest}", home.display()),
        None => target.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TempDir, seed, tree};
    use std::path::Path;

    fn refused(links: &Links) -> String {
        format!("{:#}", links.check().expect_err("refused"))
    }

    #[test]
    fn links_resolve_against_the_file_declaring_them() {
        let dir = TempDir::new("links-resolve");
        seed(&dir.join("dotfiles/nvim/init.lua"), "");
        let tree = tree(
            &dir,
            &[
                ("config.toml", "includes = [\"hosts/box.toml\"]\n"),
                (
                    "hosts/box.toml",
                    "[links]\nnvim = { source = \"../dotfiles/nvim\", target = \"/home/me/.config/nvim\" }\n",
                ),
            ],
        );
        let links = Links::load(&tree);
        links.check().expect("valid");
        let link = &links.placements[0];
        assert_eq!(link.entry, PathBuf::from("dotfiles/nvim"));
        assert_eq!(link.file, PathBuf::from("hosts/box.toml"));
        assert_eq!(link.target, PathBuf::from("/home/me/.config/nvim"));
        assert!(link.source.ends_with("dotfiles/nvim"));
    }

    #[test]
    fn a_missing_or_outside_source_and_a_bad_target_are_located() {
        let dir = TempDir::new("links-faults");
        let tree = tree(
            &dir,
            &[(
                "config.toml",
                "[links]\n\
                 a = { source = \"gone\", target = \"/x/a\" }\n\
                 b = { source = \"../../etc\", target = \"/x/b\" }\n\
                 c = { source = \"config.toml\", target = \"relative\" }\n\
                 d = { source = \"config.toml\", target = \"/x/../d\" }\n",
            )],
        );
        let shown = refused(&Links::load(&tree));
        assert!(shown.contains("`a` links nothing"), "{shown}");
        assert!(shown.contains("config.toml:2:"), "{shown}");
        assert!(
            shown.contains("`../../etc` is outside the graph"),
            "{shown}"
        );
        assert!(
            shown.contains("`relative` is not an absolute path"),
            "{shown}"
        );
        assert!(shown.contains("goes back up with `..`"), "{shown}");
    }

    #[test]
    fn a_link_declared_alike_twice_is_one_and_otherwise_refused() {
        let dir = TempDir::new("links-merge");
        seed(&dir.join("nvim/init.lua"), "");
        seed(&dir.join("vim/vimrc"), "");
        let link = "nvim = { source = \"nvim\", target = \"/x/nvim\" }\n";
        let tree = tree(
            &dir,
            &[
                (
                    "config.toml",
                    &format!("includes = [\"a.toml\"]\n[links]\n{link}"),
                ),
                ("a.toml", &format!("[links]\n{link}")),
            ],
        );
        let links = Links::load(&tree);
        links.check().expect("one link");
        assert_eq!(links.placements.len(), 1);

        seed(
            &dir.join("a.toml"),
            "[links]\nnvim = { source = \"vim\", target = \"/x/nvim\" }\n\
             vim = { source = \"vim\", target = \"/x/nvim\" }\n",
        );
        let tree = crate::testing::tree(&dir, &[]);
        let shown = refused(&Links::load(&tree));
        assert!(
            shown.contains("`nvim` is already declared differently in `config.toml`"),
            "{shown}"
        );
        assert!(
            shown.contains("is already linked by `nvim` in `config.toml`"),
            "{shown}"
        );
    }

    #[test]
    fn a_target_inside_another_link_or_the_graph_is_refused() {
        let dir = TempDir::new("links-nested");
        seed(&dir.join("nvim/init.lua"), "");
        let inside = dir.0.canonicalize().expect("real").join("x");
        let tree = tree(
            &dir,
            &[(
                "config.toml",
                &format!(
                    "[links]\nnvim = {{ source = \"nvim\", target = \"/x/nvim\" }}\n\
                     init = {{ source = \"nvim/init.lua\", target = \"/x/nvim/init.lua\" }}\n\
                     self = {{ source = \"nvim\", target = \"{}\" }}\n",
                    inside.display()
                ),
            )],
        );
        let shown = refused(&Links::load(&tree));
        assert!(
            shown.contains("is inside `/x/nvim`, which is linked too"),
            "{shown}"
        );
        assert!(shown.contains("is inside the graph"), "{shown}");

        let holder = inside
            .parent()
            .and_then(Path::parent)
            .expect("above the graph");
        let tree = crate::testing::tree(
            &dir,
            &[(
                "config.toml",
                &format!(
                    "[links]\nnvim = {{ source = \"nvim\", target = \"{}\" }}\n",
                    holder.display()
                ),
            )],
        );
        let shown = refused(&Links::load(&tree));
        assert!(shown.contains("holds the graph"), "{shown}");
    }

    #[test]
    fn only_a_folder_without_vow_files_is_checked_for_unlinked_entries() {
        let dir = TempDir::new("links-unlinked");
        seed(&dir.join("README.md"), "");
        seed(&dir.join("dotfiles/nvim/init.lua"), "");
        seed(&dir.join("dotfiles/forgotten"), "");
        let tree = tree(
            &dir,
            &[(
                "config.toml",
                "[links]\nnvim = { source = \"dotfiles/nvim\", target = \"/x/nvim\" }\n",
            )],
        );
        let links = Links::load(&tree);
        assert_eq!(links.unlinked, vec!["dotfiles/forgotten".to_string()]);
    }
}
