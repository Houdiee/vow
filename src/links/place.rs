//! Placing links: what is declared, against what is on disk. vow only
//! changes a symlink it placed; anything else in the way blocks the link.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use super::{Links, Placement, Record};
use crate::atomic;
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::report;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Already pointing at its entry.
    Keep,
    Create,
    /// The same link -- entry and target -- in a graph that has moved: the
    /// one genuine modification.
    Repoint,
    /// A link vow placed that nothing declares any more.
    Remove,
    /// A link vow placed whose target another entry now takes: a different
    /// link, so shown as removed, though the new one replaces it in one swap.
    Replaced,
    /// Something vow did not place is in the way.
    Blocked(Obstacle),
}

/// What blocks a link: anything vow did not place, which it never replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Obstacle {
    File,
    /// A directory with something in it; an empty one is replaced.
    Directory,
    ForeignLink,
}

impl Obstacle {
    pub fn reason(self) -> &'static str {
        match self {
            Obstacle::File => "a file is in the way",
            Obstacle::Directory => "a directory is in the way",
            Obstacle::ForeignLink => "a link vow did not place is in the way",
        }
    }

    fn fix(self) -> &'static str {
        match self {
            Obstacle::File | Obstacle::Directory => {
                "move it into the graph and link it from there, or delete it"
            }
            Obstacle::ForeignLink => "remove that link; vow only changes links it placed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkChange {
    pub link: Placement,
    pub action: Action,
    /// For a repoint, where the link came from until now.
    pub was: Option<Placement>,
}

impl LinkChange {
    fn new(link: &Placement, action: Action) -> Self {
        Self {
            link: link.clone(),
            action,
            was: None,
        }
    }

    /// Why a repointed link moved: the graph did, from its old source minus the entry.
    pub fn moved(&self) -> Option<String> {
        let was = self.was.as_ref()?;
        let depth = self.link.entry.components().count();
        Some(match was.source.ancestors().nth(depth) {
            Some(root) => format!("graph moved from {}", report::path(root)),
            None => "graph moved".to_string(),
        })
    }
}

/// Every declared link, and every placed one no longer declared, with what
/// `switch` would do to it.
#[derive(Debug, Default)]
pub struct LinkPlan {
    pub changes: Vec<LinkChange>,
}

impl LinkPlan {
    /// A link is its entry and target: the same pair in a moved graph is repointed,
    /// and another entry for a target vow placed is another link.
    pub fn new(links: &Links, record: &Record) -> Self {
        // A link vow placed but someone has since changed is no longer vow's.
        let removed: Vec<&Placement> = record
            .links()
            .filter(|placed| {
                !links
                    .placements
                    .iter()
                    .any(|link| link.target == placed.target)
                    && points_at(&placed.target, &placed.source)
            })
            .collect();
        let going: Vec<&Path> = removed
            .iter()
            .map(|placed| placed.target.as_path())
            .collect();

        let mut changes: Vec<LinkChange> = Vec::new();
        for link in &links.placements {
            match action(link, record, &going) {
                Action::Repoint => {
                    let placed = record.get(&link.target).expect("a repoint is recorded");
                    if placed.entry == link.entry {
                        changes.push(LinkChange {
                            was: Some(placed.clone()),
                            ..LinkChange::new(link, Action::Repoint)
                        });
                    } else {
                        changes.push(LinkChange::new(placed, Action::Replaced));
                        changes.push(LinkChange::new(link, Action::Create));
                    }
                }
                action => changes.push(LinkChange::new(link, action)),
            }
        }

        changes.extend(
            removed
                .into_iter()
                .map(|placed| LinkChange::new(placed, Action::Remove)),
        );

        // Stable, so a replaced link stays just before what replaces it.
        changes.sort_by(|left, right| left.link.target.cmp(&right.link.target));
        Self { changes }
    }

    /// The links already as declared, which a switch with nothing to change
    /// records as they are.
    pub fn kept(&self) -> Vec<Placement> {
        self.changes
            .iter()
            .filter(|change| change.action == Action::Keep)
            .map(|change| change.link.clone())
            .collect()
    }

    pub fn has_work(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.action != Action::Keep)
    }

    /// Refuse a plan with anything blocked, before anything is changed.
    pub fn check(&self) -> Result<()> {
        let mut problems = Diagnostics::default();
        for change in &self.changes {
            if let Action::Blocked(obstacle) = change.action {
                problems.push(
                    Diagnostic::error(format!(
                        "cannot link `{}`: {}",
                        report::path(&change.link.target),
                        obstacle.reason()
                    ))
                    .in_place(&change.link.target)
                    .note(obstacle.fix()),
                );
            }
        }
        problems.into_result()
    }

    /// Carry out every change, saying a line for each. Removals go first, so a
    /// directory link can give way to links inside it; each change is re-checked just
    /// before it is made, so anything changed since the plan is left alone.
    /// `previous` is the record the plan came from.
    pub fn apply(&self, previous: &Record, say: &dyn Fn(&str)) -> Applied {
        let mut applied = Applied::default();
        let (removals, rest): (Vec<_>, Vec<_>) = self
            .changes
            .iter()
            .partition(|change| change.action == Action::Remove);
        for change in removals.into_iter().chain(rest) {
            let link = &change.link;
            let outcome = match change.action {
                Action::Keep => {
                    applied.placed.push(link.clone());
                    continue;
                }
                // Gone in the swap that places its replacement.
                Action::Replaced | Action::Blocked(_) => continue,
                Action::Create | Action::Repoint => {
                    unchanged(change, previous).and_then(|()| place(link))
                }
                Action::Remove => unchanged(change, previous).and_then(|()| {
                    fs::remove_file(&link.target).with_context(|| {
                        format!("could not remove `{}`", report::path(&link.target))
                    })
                }),
            };
            applied.tried += 1;
            let row = |link: &Placement| {
                format!("{} <- {}", link.entry.display(), report::path(&link.target))
            };
            let before = previous.get(&link.target);
            match (outcome, change.action) {
                (Ok(()), Action::Remove) => say(&format!("- {}", row(link))),
                (Ok(()), Action::Repoint) => {
                    let why = change
                        .moved()
                        .map(|why| format!(" ({why})"))
                        .unwrap_or_default();
                    say(&format!("~ {}{why}", row(link)));
                    applied.placed.push(link.clone());
                }
                (Ok(()), _) => {
                    if let Some(replaced) = before.filter(|before| before.entry != link.entry) {
                        say(&format!("- {}", row(replaced)));
                    }
                    say(&format!("+ {}", row(link)));
                    applied.placed.push(link.clone());
                }
                (Err(error), _) => {
                    // Whatever vow placed there before is still there.
                    applied.placed.extend(
                        before
                            .filter(|before| points_at(&before.target, &before.source))
                            .cloned(),
                    );
                    say(&format!("! {}: {error:#}", row(link)));
                    applied.failed += 1;
                }
            }
        }
        applied
    }
}

/// What a [`LinkPlan`] left in place, and what it could not do.
#[derive(Debug, Default)]
pub struct Applied {
    /// Every link vow placed that is there now: what to record.
    pub placed: Vec<Placement>,
    /// How many changes were tried, and how many of them failed.
    pub tried: usize,
    pub failed: usize,
}

/// Put one link in place. A directory made since the plan, by a manager vow
/// ran, may go only while empty: the OS refuses to remove anything else.
fn place(link: &Placement) -> Result<()> {
    if is_real_dir(&link.target) {
        remove_empty(&link.target).map_err(|error| {
            let why = match error.kind() {
                std::io::ErrorKind::DirectoryNotEmpty => "it is no longer empty".to_string(),
                _ => error.to_string(),
            };
            anyhow::anyhow!("could not clear `{}`: {why}", report::path(&link.target))
        })?;
    }
    atomic::symlink(&link.source, &link.target)
}

/// Whether a change still finds what the plan saw: vow's link where it
/// changes or removes one, and nothing but a directory (which [`place`] only
/// clears while empty) or vow's own link where it creates one.
fn unchanged(change: &LinkChange, previous: &Record) -> Result<()> {
    let target = &change.link.target;
    let found = match change.action {
        Action::Remove => points_at(target, &change.link.source),
        Action::Repoint => change
            .was
            .as_ref()
            .is_some_and(|was| points_at(target, &was.source)),
        // Still inside a directory link vow placed, whose removal failed:
        // the path leads into the graph.
        _ if target.ancestors().skip(1).any(|dir| {
            previous
                .get(dir)
                .is_some_and(|placed| points_at(dir, &placed.source))
        }) =>
        {
            false
        }
        _ => match fs::symlink_metadata(target) {
            Err(_) => true,
            Ok(metadata) if metadata.file_type().is_symlink() => previous
                .get(target)
                .is_some_and(|placed| points_at(target, &placed.source)),
            Ok(metadata) => metadata.is_dir(),
        },
    };
    if found {
        Ok(())
    } else {
        anyhow::bail!(
            "`{}` changed since the plan; left alone",
            report::path(target)
        )
    }
}

/// What `switch` would do to one declared link. `going` holds the targets of
/// links about to be removed, which removals made first clear out of the way.
fn action(link: &Placement, record: &Record, going: &[&Path]) -> Action {
    // Inside a directory link that is going, as when it is split into links
    // for the files in it: what is there now is the graph, seen through it.
    if link
        .target
        .ancestors()
        .skip(1)
        .any(|dir| going.contains(&dir))
    {
        return Action::Create;
    }
    let Ok(metadata) = fs::symlink_metadata(&link.target) else {
        return Action::Create;
    };
    if !metadata.file_type().is_symlink() {
        // A directory holding no files has nothing to lose (`ya pkg list` makes
        // `~/.config/yazi`), nor one holding only vow's links about to go.
        if metadata.is_dir() && holds_nothing_but(&link.target, going) {
            return Action::Create;
        }
        return Action::Blocked(if metadata.is_dir() {
            Obstacle::Directory
        } else {
            Obstacle::File
        });
    }
    if points_at(&link.target, &link.source) {
        return Action::Keep;
    }
    match record.get(&link.target) {
        Some(placed) if points_at(&link.target, &placed.source) => Action::Repoint,
        _ => Action::Blocked(Obstacle::ForeignLink),
    }
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Whether a directory holds, at any depth, nothing but directories and the
/// links in `going`.
fn holds_nothing_but(dir: &Path, going: &[&Path]) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries.filter_map(|entry| entry.ok()).all(|entry| {
            let path = entry.path();
            entry.file_type().is_ok_and(|kind| {
                (kind.is_dir() && holds_nothing_but(&path, going))
                    || (kind.is_symlink() && going.contains(&path.as_path()))
            })
        })
    })
}

/// Remove a directory that holds no files, deepest first. `remove_dir`
/// refuses a directory with anything in it, so a file that appears meanwhile
/// stops this rather than being lost.
fn remove_empty(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_empty(&entry.path())?;
        }
    }
    fs::remove_dir(dir)
}

/// Whether `target` is a symlink to `source`.
fn points_at(target: &Path, source: &Path) -> bool {
    fs::read_link(target).is_ok_and(|points| points == source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TempDir, seed};
    use std::path::PathBuf;

    fn link(dir: &TempDir, entry: &str) -> Placement {
        let source = dir.join("graph").join(entry);
        seed(&source, "");
        Placement {
            source,
            entry: PathBuf::from(entry),
            target: dir.join("home").join(entry),
            file: PathBuf::from("config.toml"),
        }
    }

    fn plan(placements: Vec<Placement>, placed: Vec<Placement>) -> LinkPlan {
        let links = Links {
            placements,
            ..Links::default()
        };
        let record = Record::from(placed.as_slice());
        LinkPlan::new(&links, &record)
    }

    fn actions(plan: &LinkPlan) -> Vec<Action> {
        plan.changes.iter().map(|change| change.action).collect()
    }

    #[test]
    fn a_missing_link_is_created_and_then_kept() {
        let dir = TempDir::new("place-create");
        let nvim = link(&dir, "nvim");

        let first = plan(vec![nvim.clone()], vec![]);
        assert_eq!(actions(&first), vec![Action::Create]);
        assert!(
            first.apply(&Record::default(), &|_| {}).failed == 0,
            "applies"
        );
        assert_eq!(fs::read_link(&nvim.target).expect("linked"), nvim.source);

        let kept = plan(vec![nvim.clone()], vec![nvim]);
        assert_eq!(actions(&kept), vec![Action::Keep]);
    }

    #[test]
    fn only_a_link_vow_placed_is_replaced_repointed_or_removed() {
        let dir = TempDir::new("place-own");
        let (old, new) = (link(&dir, "old"), link(&dir, "new"));
        // Another entry for the same target: a different link.
        let other = Placement {
            target: old.target.clone(),
            ..new.clone()
        };
        // The same entry and target in a graph moved elsewhere: the same link.
        let hosted = Placement {
            source: dir.join("moved/old"),
            ..old.clone()
        };
        seed(&hosted.source, "");
        atomic::symlink(&old.source, &old.target).expect("placed");

        // Placed by vow: replaced, repointed, or removed when undeclared.
        let replaced = plan(vec![other.clone()], vec![old.clone()]);
        assert_eq!(actions(&replaced), vec![Action::Replaced, Action::Create]);
        let repointed = plan(vec![hosted], vec![old.clone()]);
        assert_eq!(actions(&repointed), vec![Action::Repoint]);
        assert_eq!(
            repointed.changes[0].moved(),
            Some(format!(
                "graph moved from {}",
                report::path(&dir.join("graph"))
            ))
        );
        assert_eq!(
            actions(&plan(vec![], vec![old.clone()])),
            vec![Action::Remove]
        );

        // Not placed by vow: in the way, and never removed.
        assert!(matches!(
            actions(&plan(vec![other], vec![]))[0],
            Action::Blocked(_)
        ));
        assert!(plan(vec![], vec![]).changes.is_empty());
    }

    #[test]
    fn a_replacement_is_one_swap_and_the_record_holds_only_the_new_link() {
        let dir = TempDir::new("place-replace");
        let old = link(&dir, "old");
        atomic::symlink(&old.source, &old.target).expect("placed");
        let new = Placement {
            target: old.target.clone(),
            ..link(&dir, "new")
        };
        let previous = Record::from(std::slice::from_ref(&old));
        let said = std::sync::Mutex::new(Vec::new());
        let applied = plan(vec![new.clone()], vec![old.clone()]).apply(&previous, &|line| {
            said.lock().expect("lock").push(line.to_string())
        });
        assert_eq!(applied.failed, 0);
        assert_eq!(applied.placed, vec![new.clone()]);
        assert_eq!(fs::read_link(&new.target).expect("linked"), new.source);
        let said = said.into_inner().expect("lines");
        assert!(
            said[0].starts_with("- old <- ") && said[1].starts_with("+ new <- "),
            "{said:?}"
        );
    }

    #[test]
    fn a_real_file_in_the_way_blocks_the_link() {
        let dir = TempDir::new("place-file");
        let gitconfig = link(&dir, "gitconfig");
        seed(&gitconfig.target, "mine");

        let blocked = plan(vec![gitconfig.clone()], vec![gitconfig.clone()]);
        assert_eq!(actions(&blocked), vec![Action::Blocked(Obstacle::File)]);
        assert!(blocked.check().is_err());
        assert!(
            blocked.apply(&Record::default(), &|_| {}).failed == 0,
            "does nothing"
        );
        assert_eq!(fs::read_to_string(&gitconfig.target).expect("kept"), "mine");
    }

    #[test]
    fn an_empty_directory_in_the_way_is_replaced_and_a_full_one_is_not() {
        let dir = TempDir::new("place-empty");
        let yazi = link(&dir, "yazi");
        fs::create_dir_all(yazi.target.join("plugins")).expect("mkdir");

        let replaced = plan(vec![yazi.clone()], vec![]);
        assert_eq!(actions(&replaced), vec![Action::Create]);
        assert!(
            replaced.apply(&Record::default(), &|_| {}).failed == 0,
            "applies"
        );
        assert_eq!(fs::read_link(&yazi.target).expect("linked"), yazi.source);

        let kept = link(&dir, "kept");
        seed(&kept.target.join("package.toml"), "mine");
        assert_eq!(
            actions(&plan(vec![kept], vec![])),
            vec![Action::Blocked(Obstacle::Directory)]
        );
    }

    /// A target whose parent is a file: no link can go there.
    fn unplaceable(dir: &TempDir, entry: &str) -> Placement {
        let mut link = link(dir, entry);
        seed(&dir.join("home/not-a-dir"), "");
        link.target = dir.join("home/not-a-dir").join(entry);
        link
    }

    #[test]
    fn a_failed_link_stops_no_other_and_what_is_placed_is_recorded() {
        let dir = TempDir::new("place-partial");
        let (a, c) = (link(&dir, "a"), link(&dir, "c"));
        let b = unplaceable(&dir, "b");
        let planned = plan(vec![a.clone(), b.clone(), c.clone()], vec![]);

        let said = std::sync::Mutex::new(Vec::new());
        let applied = planned.apply(&Record::default(), &|line| {
            said.lock().expect("lock").push(line.to_string())
        });
        assert_eq!(applied.failed, 1);
        let said = said.into_inner().expect("lines");
        // A line per link, in target order, the failure saying why.
        let said_of = |start: &str| said.iter().filter(|line| line.starts_with(start)).count();
        assert_eq!(said_of("+ a <- ") + said_of("+ c <- "), 2, "{said:?}");
        assert!(
            said.iter()
                .any(|line| line.starts_with("! b <- ") && line.contains("not-a-dir")),
            "{said:?}"
        );
        assert_eq!(fs::read_link(&a.target).expect("placed"), a.source);
        assert_eq!(fs::read_link(&c.target).expect("placed"), c.source);
        let mut recorded: Vec<PathBuf> = applied.placed.iter().map(|l| l.target.clone()).collect();
        recorded.sort();
        assert_eq!(recorded, vec![a.target.clone(), c.target.clone()]);

        // Read back, vow's own links are in place, not in the way.
        let again = plan(vec![a, c], applied.placed);
        assert_eq!(actions(&again), vec![Action::Keep, Action::Keep]);
    }

    #[test]
    fn a_failed_replacement_keeps_the_old_link_on_record() {
        let dir = TempDir::new("place-repoint-failed");
        let old = link(&dir, "old");
        atomic::symlink(&old.source, &old.target).expect("placed");
        // Declared to point elsewhere; the swap will fail.
        let new = Placement {
            target: old.target.clone(),
            ..link(&dir, "new")
        };
        let previous = Record::from(std::slice::from_ref(&old));
        let planned = plan(vec![new], vec![old.clone()]);
        assert_eq!(actions(&planned), vec![Action::Replaced, Action::Create]);
        // Something non-empty where the new link is built, so it cannot be.
        let temp = old.target.with_file_name(".old.vow-tmp");
        seed(&temp.join("in-the-way"), "");

        let applied = planned.apply(&previous, &|_| {});
        assert_eq!(applied.failed, 1);
        assert_eq!(applied.placed, vec![old.clone()]);
        assert_eq!(fs::read_link(&old.target).expect("still there"), old.source);
    }

    #[test]
    fn a_directory_link_splits_into_links_for_its_files_and_back() {
        let dir = TempDir::new("place-split");
        let whole = Placement {
            source: dir.join("graph/nvim"),
            entry: PathBuf::from("nvim"),
            target: dir.join("home/nvim"),
            file: PathBuf::from("config.toml"),
        };
        let file = Placement {
            source: dir.join("graph/nvim/init.lua"),
            entry: PathBuf::from("nvim/init.lua"),
            target: dir.join("home/nvim/init.lua"),
            file: PathBuf::from("config.toml"),
        };
        seed(&file.source, "mine");
        atomic::symlink(&whole.source, &whole.target).expect("placed");

        // Seen through the directory link, the graph's own file is no obstacle.
        let split = plan(vec![file.clone()], vec![whole.clone()]);
        assert_eq!(actions(&split), vec![Action::Remove, Action::Create]);
        let previous = Record::from(std::slice::from_ref(&whole));
        assert_eq!(split.apply(&previous, &|_| {}).failed, 0);
        assert!(is_real_dir(&whole.target));
        assert_eq!(fs::read_link(&file.target).expect("linked"), file.source);

        // A directory holding only vow's links that are going is no obstacle.
        let joined = plan(vec![whole.clone()], vec![file.clone()]);
        assert_eq!(actions(&joined), vec![Action::Create, Action::Remove]);
        let previous = Record::from(std::slice::from_ref(&file));
        assert_eq!(joined.apply(&previous, &|_| {}).failed, 0);
        assert_eq!(fs::read_link(&whole.target).expect("linked"), whole.source);
        assert_eq!(fs::read_to_string(&file.source).expect("kept"), "mine");
    }

    #[test]
    fn what_changes_while_the_plan_is_read_is_left_alone() {
        let dir = TempDir::new("place-changed");
        let (gitconfig, old) = (link(&dir, "gitconfig"), link(&dir, "old"));
        atomic::symlink(&old.source, &old.target).expect("placed");
        let previous = Record::from(std::slice::from_ref(&old));
        let planned = plan(vec![gitconfig.clone()], vec![old.clone()]);
        assert_eq!(actions(&planned), vec![Action::Create, Action::Remove]);

        // The user writes a file where the link goes, and repoints vow's old link.
        seed(&gitconfig.target, "mine");
        atomic::symlink(&dir.join("theirs"), &old.target).expect("relinked");

        let applied = planned.apply(&previous, &|_| {});
        assert_eq!(applied.failed, 2);
        assert_eq!(fs::read_to_string(&gitconfig.target).expect("kept"), "mine");
        assert_eq!(
            fs::read_link(&old.target).expect("kept"),
            dir.join("theirs")
        );
        assert!(applied.placed.is_empty());
    }

    #[test]
    fn a_directory_made_after_the_plan_is_replaced_only_while_empty() {
        let dir = TempDir::new("place-raced");
        let yazi = link(&dir, "yazi");
        let planned = plan(vec![yazi.clone()], vec![]);

        // A manager run between planning and applying makes the directory.
        fs::create_dir_all(&yazi.target).expect("mkdir");
        assert!(
            planned.apply(&Record::default(), &|_| {}).failed == 0,
            "an empty directory goes"
        );
        assert_eq!(fs::read_link(&yazi.target).expect("linked"), yazi.source);

        let full = link(&dir, "full");
        let planned = plan(vec![full.clone()], vec![]);
        seed(&full.target.join("package.toml"), "mine");
        assert!(
            planned.apply(&Record::default(), &|_| {}).failed == 1,
            "a file appeared: nothing is removed"
        );
        assert_eq!(
            fs::read_to_string(full.target.join("package.toml")).expect("kept"),
            "mine"
        );
    }
}
