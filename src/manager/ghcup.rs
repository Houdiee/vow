use crate::exec::Invocation;
use crate::manager::{Manager, parse_two_column};
use crate::packages::PackageSpec;

/// ghcup manages the Haskell toolchain, converged by the version *set* per tool.
pub struct Ghcup;

impl Manager for Ghcup {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "ghcup"
    }

    /// Install, then set: `install` alone leaves the active version as it was.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .flat_map(|spec| {
                let name = spec.name.as_str();
                let version = spec.version.as_deref().unwrap_or("latest");
                [
                    Invocation::new("ghcup")
                        .arg("install")
                        .arg(name)
                        .arg(version),
                    Invocation::new("ghcup").arg("set").arg(name).arg(version),
                ]
            })
            .collect()
    }

    /// `ghcup rm TOOL VERSION` has no name-only form, so a tool listed without
    /// a version is left alone.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .filter_map(|spec| {
                let version = spec.version.as_deref()?;
                Some(
                    Invocation::new("ghcup")
                        .arg("rm")
                        .arg(spec.name.as_str())
                        .arg(version),
                )
            })
            .collect()
    }

    /// `-c set`: one row per tool, unlike `-c installed`; `-r` for raw columns.
    fn list_command(&self) -> Invocation {
        Invocation::new("ghcup").args(["list", "-c", "set", "-r"])
    }

    /// `tool version tags [date]`; the tags and date are dropped.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// GHCup's metadata keeps older releases available.
    fn supports_pinning(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `ghcup list -c set -r` with ghc 9.10.3 set and 9.8.2 installed.
    const LIST: &str = "\
cabal 3.16.1.0 recommended 
ghc 9.10.3 recommended,base-4.20.1.0 
ghcup 0.2.6.2 latest,recommended 
stack 3.11.1 latest,recommended 
";

    #[test]
    fn each_tool_and_its_set_version_are_parsed() {
        assert_eq!(
            Ghcup.parse_list(LIST),
            vec![
                PackageSpec::pinned("cabal", "3.16.1.0"),
                PackageSpec::pinned("ghc", "9.10.3"),
                PackageSpec::pinned("ghcup", "0.2.6.2"),
                PackageSpec::pinned("stack", "3.11.1"),
            ]
        );
    }

    #[test]
    fn only_the_set_version_is_listed() {
        assert_eq!(Ghcup.list_command().args, vec!["list", "-c", "set", "-r"]);
    }

    #[test]
    fn a_removal_names_the_version_it_removes() {
        let commands = Ghcup.uninstall_commands(&[PackageSpec::pinned("ghc", "9.8.2")]);
        assert_eq!(commands[0].args, vec!["rm", "ghc", "9.8.2"]);
    }

    #[test]
    fn a_tool_with_no_known_version_is_left_alone() {
        assert!(
            Ghcup
                .uninstall_commands(&[PackageSpec::new("ghc")])
                .is_empty()
        );
    }

    #[test]
    fn installing_sets_the_version_as_well() {
        let commands = Ghcup.install_commands(&[PackageSpec::pinned("ghc", "9.10.3")]);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].args, vec!["install", "ghc", "9.10.3"]);
        assert_eq!(commands[1].args, vec!["set", "ghc", "9.10.3"]);
    }
}
