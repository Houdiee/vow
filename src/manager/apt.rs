use crate::exec::Invocation;
use crate::manager::{Manager, batched, pinned_with};
use crate::packages::PackageSpec;

pub struct Apt;

impl Manager for Apt {
    fn stage(&self) -> super::Stage {
        super::Stage::System
    }

    fn id(&self) -> &'static str {
        "apt"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        // `apt-get`, as apt's CLI is unstable for scripts; `-y` as vow has asked already.
        batched(
            Invocation::new("apt-get")
                .with_root()
                .args(["install", "-y"]),
            packages.iter().map(|spec| pinned_with(spec, "=")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        // dpkg refuses a package something still needs, unlike `apt-get remove`,
        // which also removes its dependents. `--remove` keeps configuration.
        batched(
            Invocation::new("dpkg").with_root().arg("--remove"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("apt-get")
                .with_root()
                .args(["install", "--only-upgrade", "-y"]),
            unpinned.iter().cloned(),
        )
    }

    fn outdated_command(&self) -> Option<Invocation> {
        Some(Invocation::new("apt").args(["list", "--upgradable"]))
    }

    /// `name/suite available arch [upgradable from: installed]` after `Listing...`.
    fn parse_outdated(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter(|line| line.contains('/') && line.contains('['))
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let name = fields.next()?.split('/').next()?;
                let available = fields.next()?;
                Some(PackageSpec::pinned(name, available))
            })
            .collect()
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("apt-cache").args(["search"]).arg(query))
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("apt-mark").arg("showmanual")
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let name = line.split_whitespace().next()?;
                Some(PackageSpec::new(name))
            })
            .collect()
    }

    /// Debian and Ubuntu prune old versions from their archives.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `apt list --upgradable` in debian:stable-slim.
    const OUTDATED: &str = "\
Listing...
libpcre2-8-0/stable-security 10.46-1~deb13u3 amd64 [upgradable from: 10.46-1~deb13u2]
libssl3t64/stable-security 3.5.7-1~deb13u3 amd64 [upgradable from: 3.5.7-1~deb13u2]
";

    #[test]
    fn showmanual_output_is_parsed() {
        let out = "git\nripgrep\nvim\n";
        assert_eq!(
            Apt.parse_list(out),
            vec![
                PackageSpec::new("git"),
                PackageSpec::new("ripgrep"),
                PackageSpec::new("vim")
            ]
        );
    }

    #[test]
    fn pins_use_equals_syntax() {
        let cmds = Apt.install_commands(&[
            PackageSpec::pinned("ripgrep", "14.1.0"),
            PackageSpec::new("vim"),
        ]);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].args, vec!["install", "-y", "ripgrep=14.1.0", "vim"]);
    }

    #[test]
    fn removal_neither_purges_nor_takes_what_depends_on_it() {
        let names = vec![PackageSpec::new("vim")];
        let cmd = &Apt.uninstall_commands(&names)[0];
        assert_eq!(
            (cmd.program.as_str(), cmd.args[0].as_str()),
            ("dpkg", "--remove")
        );
    }

    #[test]
    fn install_and_remove_need_root() {
        assert!(Apt.install_commands(&[PackageSpec::new("vim")])[0].needs_root);
        assert!(Apt.uninstall_commands(&[PackageSpec::new("vim")])[0].needs_root);
    }

    #[test]
    fn the_suite_is_stripped_and_the_header_skipped() {
        assert_eq!(
            Apt.parse_outdated(OUTDATED),
            vec![
                PackageSpec::pinned("libpcre2-8-0", "10.46-1~deb13u3"),
                PackageSpec::pinned("libssl3t64", "3.5.7-1~deb13u3"),
            ]
        );
    }
}
