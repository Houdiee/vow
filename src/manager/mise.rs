use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column, pinned_with};
use crate::packages::PackageSpec;

/// mise installs versions of tools: names, each optionally pinned.
pub struct Mise;

impl Manager for Mise {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "mise"
    }

    /// `use --global`, unlike `install`, makes the version the active one vow reads back.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("mise").args(["use", "--global"]),
            packages.iter().map(|spec| pinned_with(spec, "@")),
        )
    }

    /// Undoes `use --global`, pruning versions nothing else uses; a plain
    /// `uninstall` would leave the tool in the global config, listed as missing.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("mise").args(["unuse", "--global"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("mise").arg("upgrade"),
            unpinned.iter().cloned(),
        )
    }

    /// `--current`: one row per tool, unlike `--installed`, which lists every version.
    fn list_command(&self) -> Invocation {
        Invocation::new("mise").args(["ls", "--current"])
    }

    /// `tool version <source> <requested>`; the first two columns are read.
    /// A configured version not installed is marked `(missing)`, and is not there.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        let present: Vec<&str> = stdout
            .lines()
            .filter(|line| !line.contains("(missing)"))
            .collect();
        parse_two_column(&present.join("\n"))
    }

    /// A tool has one active version, and `mise use --global` moves it.
    fn supports_pinning(&self) -> bool {
        true
    }

    /// `mise ls --current` says nothing at all before the first install.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `mise ls --current` after `mise use -g node@22`.
    const LIST: &str = "node  22.23.3  ~/.config/mise/config.toml  22\npython  3.13.1\n";

    #[test]
    fn the_tool_and_its_version_are_parsed() {
        assert_eq!(
            Mise.parse_list(LIST),
            vec![
                PackageSpec::pinned("node", "22.23.3"),
                PackageSpec::pinned("python", "3.13.1"),
            ]
        );
    }

    #[test]
    fn a_configured_version_not_installed_is_not_listed() {
        // Captured after `mise uninstall --all node` left node in the global config.
        let listing = "node  22.11.0 (missing)  ~/.config/mise/config.toml  22.11.0\n";
        assert!(Mise.parse_list(listing).is_empty());
    }

    #[test]
    fn removal_takes_the_tool_out_of_the_global_config() {
        let commands = Mise.uninstall_commands(&[PackageSpec::pinned("node", "22.11.0")]);
        assert_eq!(commands[0].args, vec!["unuse", "--global", "node"]);
    }

    #[test]
    fn a_pin_moves_the_active_version() {
        let commands = Mise.install_commands(&[PackageSpec::pinned("node", "20.18.0")]);
        assert_eq!(
            commands[0].args,
            vec!["use", "--global", "node@20.18.0"],
            "a plain `install` would add a version without activating it"
        );
    }

    #[test]
    fn only_the_active_version_is_listed() {
        assert_eq!(Mise.list_command().args, vec!["ls", "--current"]);
    }
}
