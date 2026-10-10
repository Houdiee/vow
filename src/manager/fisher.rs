use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

/// fisher installs fish plugins by repository, so a name is an `owner/repo`.
pub struct Fisher;

impl Manager for Fisher {
    fn id(&self) -> &'static str {
        "fisher"
    }

    /// fisher is a fish function, not an executable, so fish is probed.
    fn binary(&self) -> &'static str {
        "fish"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("fish").args(["-c", "fisher install $argv"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("fish").args(["-c", "fisher remove $argv"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("fish").args(["-c", "fisher update $argv"]),
            unpinned.iter().cloned(),
        )
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("fish").args(["-c", "fisher list"])
    }

    /// One `owner/repo` per line. Plugins from a local path are left out:
    /// nothing portable could reinstall them.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('/'))
            .map(PackageSpec::new)
            .collect()
    }

    /// fisher reports no version, so a pin could never be checked.
    fn supports_pinning(&self) -> bool {
        false
    }

    /// `fisher list` says nothing until fisher has installed itself.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `fisher list` after bootstrapping fisher in Debian.
    const LIST: &str = "jorgebucaran/fisher\nIlanCosman/tide@v6\n";

    #[test]
    fn repositories_are_the_names() {
        assert_eq!(
            Fisher.parse_list(LIST),
            vec![
                PackageSpec::new("jorgebucaran/fisher"),
                PackageSpec::new("IlanCosman/tide@v6"),
            ]
        );
    }

    #[test]
    fn a_branch_suffix_stays_part_of_the_name() {
        let tide = &Fisher.parse_list(LIST)[1];
        assert_eq!(tide.name, "IlanCosman/tide@v6");
        assert!(tide.version.is_none());
    }

    #[test]
    fn a_locally_pathed_plugin_is_not_declared() {
        assert!(Fisher.parse_list("/home/me/dev/myplugin\n").is_empty());
    }
}
