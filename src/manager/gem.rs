use crate::exec::Invocation;
use crate::manager::{Manager, batched, pinned_with};
use crate::packages::PackageSpec;

pub struct Gem;

impl Manager for Gem {
    fn id(&self) -> &'static str {
        "gem"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("gem").args(["install", "--no-document"]),
            packages.iter().map(|spec| pinned_with(spec, ":")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        // Without these, uninstall stops to ask which version to remove.
        batched(
            Invocation::new("gem").args(["uninstall", "--executables", "--all"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("gem").args(["update", "--no-document"]),
            unpinned.iter().cloned(),
        )
    }

    fn outdated_command(&self) -> Option<Invocation> {
        Some(Invocation::new("gem").arg("outdated"))
    }

    /// `name (installed < available)`
    fn parse_outdated(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let (name, rest) = line.split_once(" (")?;
                let available = rest.split('<').nth(1)?.trim_end_matches(')').trim();
                Some(PackageSpec::pinned(name.trim(), available))
            })
            .collect()
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(
            Invocation::new("gem")
                .args(["search", "--remote"])
                .arg(query),
        )
    }

    /// `-d` for the `Installed at:` line; see [`parse_installed`].
    fn list_command(&self) -> Invocation {
        Invocation::new("gem").args(["list", "--local", "-d"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_installed(stdout)
    }

    /// RubyGems keeps versions side by side, so a pin would never converge:
    /// vow would see the newest and report the same drift forever.
    fn supports_pinning(&self) -> bool {
        false
    }
}

/// Gems someone asked for, from `gem list --local -d`: Ruby's own directory, which holds
/// its default and bundled gems, is left out (set `GEM_HOME` to install elsewhere).
fn parse_installed(stdout: &str) -> Vec<PackageSpec> {
    let shipped = stdout.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Installed at (default):")?;
        Some(rest.trim())
    });

    let mut found = Vec::new();
    let mut header: Option<PackageSpec> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("Installed at") else {
            // A gem's header starts at column zero; its fields are indented.
            if !trimmed.is_empty() && !line.starts_with(char::is_whitespace) {
                header = parse_header(trimmed);
            }
            continue;
        };
        if rest.starts_with(" (default)") {
            header = None;
            continue;
        }
        // `Installed at: <path>`, or `Installed at (2.3.0): <path>`.
        if let Some((_, path)) = rest.split_once(": ")
            && let Some(spec) = header.take()
            && Some(path.trim()) != shipped
        {
            found.push(spec);
        }
    }

    found
}

/// `name (version)`, or `name (newest, older)` where several are installed.
fn parse_header(line: &str) -> Option<PackageSpec> {
    let (name, rest) = line.split_once(" (")?;
    let version = rest.trim_end_matches(')').split(',').next()?.trim();
    Some(PackageSpec::pinned(name.trim(), version))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `gem outdated` in ruby:slim.
    const OUTDATED: &str = "\
bigdecimal (4.0.1 < 4.1.3)
tilt (2.0.11 < 2.3.0)
";

    // Captured from `gem list --local -d` with `GEM_HOME=/gems`: `colorize` and `tilt`
    // (twice) installed there, `csv` and `json` shipped with Ruby.
    const LIST: &str = "\
colorize (1.1.0)
    Author: Michał Kalbarczyk
    License: GPL-2.0
    Installed at: /gems

    Ruby gem for colorizing text using ANSI escape sequences.
tilt (2.3.0, 2.0.11)
    Authors: Ryan Tomayko, Magnus Holm, Jeremy Evans
    License: MIT
    Installed at (2.3.0): /gems
                 (2.0.11): /gems

    Generic interface to multiple Ruby template engines
csv (3.3.2)
    Licenses: Ruby, BSD-2-Clause
    Installed at: /nix/store/abc-ruby-3.4.9/lib/ruby/gems/3.4.0

    CSV Reading and Writing
json (2.9.1)
    License: Ruby
    Installed at (default): /nix/store/abc-ruby-3.4.9/lib/ruby/gems/3.4.0

    JSON Implementation for Ruby
";

    #[test]
    fn only_gems_outside_rubys_own_tree_are_declared() {
        let names: Vec<String> = Gem
            .parse_list(LIST)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names, vec!["colorize", "tilt"]);
    }

    #[test]
    fn a_bundled_gem_is_not_a_removal_candidate() {
        // Removing it would dismantle the Ruby installation.
        let names: Vec<String> = Gem
            .parse_list(LIST)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert!(!names.contains(&"csv".to_string()));
        assert!(!names.contains(&"json".to_string()));
    }

    #[test]
    fn a_gem_with_several_versions_takes_the_newest() {
        let parsed = Gem.parse_list(LIST);
        let tilt = parsed
            .iter()
            .find(|spec| spec.name == "tilt")
            .expect("tilt is listed despite its per-version install lines");
        assert_eq!(tilt.version.as_deref(), Some("2.3.0"));
    }

    #[test]
    fn removal_does_not_stop_to_ask() {
        let commands = Gem.uninstall_commands(&[PackageSpec::new("tilt")]);
        assert_eq!(
            commands[0].args,
            vec!["uninstall", "--executables", "--all", "tilt"]
        );
    }

    #[test]
    fn the_version_after_the_less_than_is_the_one_available() {
        assert_eq!(
            Gem.parse_outdated(OUTDATED),
            vec![
                PackageSpec::pinned("bigdecimal", "4.1.3"),
                PackageSpec::pinned("tilt", "2.3.0"),
            ]
        );
    }
}
