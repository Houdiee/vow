use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column};
use crate::packages::PackageSpec;

pub struct Arch;

/// pacman, paru and yay share one local database, so an AUR helper is how Arch
/// installs, not another manager: as two, each would see the other's packages as drift.
enum Installer {
    /// `paru` or `yay`, which call `sudo` themselves, so must not be run through it.
    Helper(&'static str),
    Pacman,
}

impl Installer {
    fn detect() -> Self {
        for helper in ["paru", "yay"] {
            if which::which(helper).is_ok() {
                return Installer::Helper(helper);
            }
        }
        Installer::Pacman
    }

    fn command(&self) -> Invocation {
        match self {
            Installer::Helper(helper) => Invocation::new(helper),
            Installer::Pacman => Invocation::new("pacman").with_root(),
        }
    }
}

impl Manager for Arch {
    fn stage(&self) -> super::Stage {
        super::Stage::System
    }

    fn id(&self) -> &'static str {
        "pacman"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        // `--needed` keeps a satisfied package from being reinstalled on a retry.
        batched(
            Installer::detect()
                .command()
                .args(["-S", "--needed", "--noconfirm"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        // `-Rs` removes orphaned dependencies but, unlike `-Rns`, keeps configuration.
        batched(
            Invocation::new("pacman")
                .with_root()
                .args(["-Rs", "--noconfirm"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    /// Arch supports no partial upgrade, so an upgrade is always the whole system.
    fn upgrade_commands(&self, _unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        vec![Installer::detect().command().args(["-Syu", "--noconfirm"])]
    }

    fn outdated_command(&self) -> Option<Invocation> {
        Some(Invocation::new("pacman").arg("-Qu"))
    }

    /// `name installed -> available`
    fn parse_outdated(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let (name, rest) = line.split_once(char::is_whitespace)?;
                let available = rest.split("-> ").nth(1)?.split_whitespace().next()?;
                Some(PackageSpec::pinned(name.trim(), available))
            })
            .collect()
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("pacman").args(["-Ss"]).arg(query))
    }

    fn list_command(&self) -> Invocation {
        // `-Qe`: explicit installs only, so dependencies never show as drift.
        Invocation::new("pacman").arg("-Qe")
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `pacman -Qu` in archlinux:latest.
    const OUTDATED: &str = "\
coreutils 9.11-2 -> 9.12-2
glibc 2.44+r24+g16be1518495f-1 -> 2.44+r50+g1848099f063e-1
";

    const QE: &str = "\
git 2.46.0-1
linux 6.10.6.arch1-1
ripgrep 14.1.0-1
";

    #[test]
    fn upgradable_packages_are_parsed_with_the_version_available() {
        assert_eq!(
            Arch.parse_outdated(OUTDATED),
            vec![
                PackageSpec::pinned("coreutils", "9.12-2"),
                PackageSpec::pinned("glibc", "2.44+r50+g1848099f063e-1"),
            ]
        );
    }

    #[test]
    fn an_upgrade_is_the_whole_system_never_named_packages() {
        let commands = Arch.upgrade_commands(&["bat".to_string()], &[]);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].args, vec!["-Syu", "--noconfirm"]);
    }

    #[test]
    fn explicit_packages_are_parsed_with_versions() {
        assert_eq!(
            Arch.parse_list(QE),
            vec![
                PackageSpec::pinned("git", "2.46.0-1"),
                PackageSpec::pinned("linux", "6.10.6.arch1-1"),
                PackageSpec::pinned("ripgrep", "14.1.0-1"),
            ]
        );
    }

    #[test]
    fn listing_always_uses_pacman() {
        let list = Arch.list_command();
        assert_eq!(list.program, "pacman");
        assert!(!list.needs_root, "a query needs no privileges");
    }

    #[test]
    fn removal_keeps_configuration() {
        let commands = Arch.uninstall_commands(&[PackageSpec::new("vim")]);
        assert_eq!(commands[0].args[0], "-Rs");
        assert!(commands[0].needs_root);
    }

    #[test]
    fn unpinned_packages_share_one_command() {
        let commands = Arch.install_commands(&[PackageSpec::new("vim"), PackageSpec::new("git")]);
        assert_eq!(commands.len(), 1);
        assert_eq!(&commands[0].args[..2], &["-S", "--needed"]);
    }
}
