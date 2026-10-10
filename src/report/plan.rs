//! How a package plan reads: a section per manager, then one summary line.

use std::cmp::Ordering;

use super::style::{added, bold, changed, dim, problem, removed};
use super::{marked, nested};
use crate::packages::{Convergence, Repin};

/// Print one manager's changes, followed by a blank line; nothing when it has
/// none.
pub fn plan(changes: &Convergence) {
    if changes.has_work() {
        println!("{}", render(changes));
    }
}

fn render(changes: &Convergence) -> String {
    if !changes.has_work() {
        return String::new();
    }

    let installs = changes
        .install
        .iter()
        .map(|spec| marked(Some(added("+")), spec));
    let repins = changes.repin.iter().map(|change| {
        marked(
            Some(changed("~")),
            format!("{} {}", change.spec, direction(change)),
        )
    });
    let removals = changes
        .remove
        .iter()
        .map(|spec| marked(Some(removed("-")), spec));

    let mut out = format!("{}\n", bold(&changes.manager));
    for row in installs.chain(repins).chain(removals) {
        out.push_str(&nested(1, row));
        out.push('\n');
    }
    out
}

/// A downgrade is called one, so it is not approved as an upgrade.
fn direction(change: &Repin) -> String {
    match change.direction() {
        Some(Ordering::Less) => problem(&format!("(downgrade from {})", change.installed)),
        Some(Ordering::Greater) => dim(&format!("(upgrade from {})", change.installed)),
        _ => dim(&format!("(installed {})", change.installed)),
    }
}

/// Counts across every manager, with repins split by direction; empty when
/// there is nothing to change.
pub fn summarize(sets: &[Convergence]) -> String {
    let install: usize = sets.iter().map(|changes| changes.install.len()).sum();
    let remove: usize = sets.iter().map(|changes| changes.remove.len()).sum();
    let (mut up, mut down, mut sideways) = (0, 0, 0);
    for change in sets.iter().flat_map(|changes| &changes.repin) {
        match change.direction() {
            Some(Ordering::Greater) => up += 1,
            Some(Ordering::Less) => down += 1,
            _ => sideways += 1,
        }
    }

    let parts: Vec<String> = [
        (install, "to install", added as fn(&str) -> String),
        (up, "to upgrade", changed),
        (down, "to downgrade", changed),
        (sideways, "to change", changed),
        (remove, "to remove", removed),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, what, paint)| paint(&format!("{count} {what}")))
    .collect();
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::PackageSpec;

    #[test]
    fn every_kind_of_change_is_shown_and_counted() {
        let changes = Convergence {
            manager: "cargo".into(),
            install: vec![PackageSpec::new("ripgrep")],
            repin: vec![
                Repin {
                    spec: PackageSpec::pinned("bat", "0.24.0"),
                    installed: "0.23.0".into(),
                },
                Repin {
                    spec: PackageSpec::pinned("hexyl", "0.13.1"),
                    installed: "0.14.0".into(),
                },
            ],
            remove: vec![PackageSpec::new("nano")],
            unremovable: Vec::new(),
        };
        assert_eq!(
            render(&changes),
            "cargo\n  + ripgrep\n  ~ bat 0.24.0 (upgrade from 0.23.0)\n  \
             ~ hexyl 0.13.1 (downgrade from 0.14.0)\n  - nano\n"
        );
        assert_eq!(
            summarize(&[changes]),
            "1 to install, 1 to upgrade, 1 to downgrade, 1 to remove"
        );
    }

    #[test]
    fn nothing_to_do_renders_nothing() {
        let quiet = Convergence::default();
        assert!(render(&quiet).is_empty());
        assert!(summarize(&[quiet]).is_empty());
    }
}
