//! How links read: under one `links` heading, a row each, in a plan and in
//! the listing alike.

use super::style::{added, bold, changed, dim, failure, removed};
use super::{item, marked};
use crate::links::{Action, LinkChange, LinkPlan};

/// The links that would change, then a summary.
pub fn links(plan: &LinkPlan) {
    let shown: Vec<&LinkChange> = plan
        .changes
        .iter()
        .filter(|change| change.action != Action::Keep)
        .collect();
    section(&shown, false);
    println!("{}", summary(&plan.changes));
}

/// Every link, each with the file declaring it, then a summary.
pub fn every_link(plan: &LinkPlan) {
    let shown: Vec<&LinkChange> = plan.changes.iter().collect();
    section(&shown, true);
    println!("{}", summary(&plan.changes));
}

fn section(shown: &[&LinkChange], with_file: bool) {
    let widths = Widths::of(shown);
    println!("{}", bold("links"));
    for change in shown {
        item(1, row(change, &widths, with_file));
    }
}

/// Entries kept in the graph that no link places.
pub fn unlinked(entries: &[String]) {
    if entries.is_empty() {
        return;
    }
    println!();
    println!("{}", bold("not linked"));
    for name in entries {
        item(1, name.clone());
    }
}

/// The widest entry and target, so the columns line up.
struct Widths {
    entry: usize,
    target: usize,
}

impl Widths {
    fn of(changes: &[&LinkChange]) -> Self {
        let widest = |text: &dyn Fn(&LinkChange) -> String| {
            changes
                .iter()
                .map(|change| text(change).chars().count())
                .max()
                .unwrap_or(0)
        };
        Self {
            entry: widest(&|change| change.link.entry.display().to_string()),
            target: widest(&|change| super::path(&change.link.target)),
        }
    }
}

fn row(change: &LinkChange, widths: &Widths, with_file: bool) -> String {
    let mark = match change.action {
        Action::Keep => None,
        Action::Create => Some(added("+")),
        Action::Repoint => Some(changed("~")),
        Action::Remove | Action::Replaced => Some(removed("-")),
        Action::Blocked(_) => Some(failure("!")),
    };
    // Padded before painting: escape codes would count toward the width.
    let entry = format!(
        "{:<width$}",
        change.link.entry.display().to_string(),
        width = widths.entry
    );
    let target = super::path(&change.link.target);
    let mut text = format!("{entry}  {} ", dim("<-"));
    if with_file {
        let file = change.link.file.display().to_string();
        text.push_str(&format!(
            "{target:<width$}  {}",
            dim(&file),
            width = widths.target
        ));
    } else {
        text.push_str(&target);
    }
    let why = match change.action {
        Action::Blocked(obstacle) => Some(obstacle.reason().to_string()),
        Action::Repoint => change.moved(),
        _ => None,
    };
    if let Some(why) = why {
        text.push_str(&format!(" {}", dim(&format!("({why})"))));
    }
    marked(mark, text)
}

fn summary(changes: &[LinkChange]) -> String {
    let count = |wanted: fn(&Action) -> bool| {
        changes
            .iter()
            .filter(|change| wanted(&change.action))
            .count()
    };
    let parts: Vec<String> = [
        (
            count(|a| *a == Action::Create),
            "to link",
            added as fn(&str) -> String,
        ),
        (count(|a| *a == Action::Repoint), "to repoint", changed),
        (
            count(|a| matches!(a, Action::Remove | Action::Replaced)),
            "to unlink",
            removed,
        ),
        (
            count(|a| matches!(a, Action::Blocked(_))),
            "blocked",
            failure,
        ),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, what, paint)| paint(&format!("{count} {what}")))
    .collect();

    if parts.is_empty() {
        return dim("every link in place");
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::{Obstacle, Placement};
    use std::path::PathBuf;

    fn change(target: &str, entry: &str, action: Action) -> LinkChange {
        LinkChange {
            link: Placement {
                source: PathBuf::from("/graph").join(entry),
                entry: PathBuf::from(entry),
                target: PathBuf::from(target),
                file: PathBuf::from("config.toml"),
            },
            action,
            was: None,
        }
    }

    #[test]
    fn rows_line_up_with_and_without_a_marker() {
        let widths = Widths {
            entry: 4,
            target: 7,
        };
        assert_eq!(
            row(&change("/etc/x", "nvim", Action::Keep), &widths, false),
            "  nvim  <- /etc/x"
        );
        assert_eq!(
            row(&change("/etc/xy", "vim", Action::Create), &widths, false),
            "+ vim   <- /etc/xy"
        );
        assert_eq!(
            row(
                &change("/etc/z", "rc", Action::Blocked(Obstacle::File)),
                &widths,
                false
            ),
            "! rc    <- /etc/z (a file is in the way)"
        );
    }

    #[test]
    fn a_listing_names_each_file_in_a_column() {
        let (nvim, git) = (
            change("/etc/nvim", "nvim", Action::Keep),
            change("/etc/gitconfig", "git", Action::Create),
        );
        let widths = Widths::of(&[&nvim, &git]);
        assert_eq!(
            row(&nvim, &widths, true),
            "  nvim  <- /etc/nvim       config.toml"
        );
        assert_eq!(
            row(&git, &widths, true),
            "+ git   <- /etc/gitconfig  config.toml"
        );
    }

    #[test]
    fn the_summary_counts_each_kind_of_change() {
        let changes = [
            change("/a", "a", Action::Create),
            change("/b", "b", Action::Keep),
            change("/c", "c", Action::Remove),
            change("/d", "d", Action::Create),
        ];
        assert_eq!(summary(&changes), "2 to link, 1 to unlink");
        assert_eq!(summary(&changes[1..2]), "every link in place");
    }
}
