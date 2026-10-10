use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column};
use crate::packages::PackageSpec;

pub struct Dnf;

impl Manager for Dnf {
    fn stage(&self) -> super::Stage {
        super::Stage::System
    }

    fn id(&self) -> &'static str {
        "dnf"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("dnf").with_root().args(["install", "-y"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    /// `dnf remove` also removes whatever depends on a package, which may be
    /// declared; rpm refuses a package something still needs.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("rpm").with_root().arg("--erase"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("dnf").with_root().args(["upgrade", "-y"]),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("dnf").arg("search").arg(query))
    }

    /// `--userinstalled` leaves out dependencies; `--qf` fixes the format.
    fn list_command(&self) -> Invocation {
        Invocation::new("dnf").args(["repoquery", "--userinstalled", "--qf", "%{name} %{evr}\\n"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// Fedora retires old builds from its repositories.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_never_takes_what_depends_on_it() {
        let cmd = &Dnf.uninstall_commands(&[PackageSpec::new("vim")])[0];
        assert_eq!(
            (cmd.program.as_str(), cmd.args[0].as_str()),
            ("rpm", "--erase")
        );
    }

    // Captured from `dnf repoquery --userinstalled` in fedora:latest.
    const LIST: &str = "\
bash 5.3.9-3.fc44
bzip2 1.0.8-23.fc44
coreutils 9.10-5.fc44
ripgrep 15.2.0-1.fc44
";

    #[test]
    fn the_forced_format_parses_as_two_columns() {
        assert_eq!(
            Dnf.parse_list(LIST),
            vec![
                PackageSpec::pinned("bash", "5.3.9-3.fc44"),
                PackageSpec::pinned("bzip2", "1.0.8-23.fc44"),
                PackageSpec::pinned("coreutils", "9.10-5.fc44"),
                PackageSpec::pinned("ripgrep", "15.2.0-1.fc44"),
            ]
        );
    }

    #[test]
    fn the_query_format_is_passed_through_verbatim() {
        // dnf interprets the escape itself; it must not arrive already expanded.
        assert_eq!(
            Dnf.list_command().args.last().expect("format"),
            "%{name} %{evr}\\n"
        );
    }

    #[test]
    fn writes_need_root_and_queries_do_not() {
        assert!(Dnf.install_commands(&[PackageSpec::new("ripgrep")])[0].needs_root);
        assert!(Dnf.uninstall_commands(&[PackageSpec::new("ripgrep")])[0].needs_root);
        assert!(!Dnf.list_command().needs_root);
    }

    #[test]
    fn an_upgrade_names_only_the_unpinned() {
        let commands = Dnf.upgrade_commands(&["bat".to_string()], &["ripgrep".to_string()]);
        assert_eq!(commands[0].args, vec!["upgrade", "-y", "bat"]);
    }
}
