use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column};
use crate::packages::PackageSpec;

pub struct Flatpak;

impl Manager for Flatpak {
    fn id(&self) -> &'static str {
        "flatpak"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("flatpak").args(["install", "--noninteractive"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("flatpak").args(["uninstall", "--noninteractive"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("flatpak").args(["search"]).arg(query))
    }

    fn list_command(&self) -> Invocation {
        // --app leaves out runtimes, which are dependencies.
        Invocation::new("flatpak").args(["list", "--app", "--columns=application,version"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// A ref, `org.app.Name//stable` or `org.app.Name/x86_64/stable`, is
    /// listed by its application id.
    fn canonical(&self, name: &str) -> String {
        name.split('/').next().unwrap_or(name).to_string()
    }

    /// Flatpak addresses builds by commit, not by version.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "\
org.gnome.Calculator\t46.1
com.visualstudio.code\t1.89.1
";

    #[test]
    fn application_ids_and_versions_are_parsed() {
        assert_eq!(
            Flatpak.parse_list(LIST),
            vec![
                PackageSpec::pinned("org.gnome.Calculator", "46.1"),
                PackageSpec::pinned("com.visualstudio.code", "1.89.1"),
            ]
        );
    }

    #[test]
    fn installs_do_not_stop_to_ask() {
        let commands = Flatpak.install_commands(&[PackageSpec::new("org.gnome.Calculator")]);
        assert_eq!(
            commands[0].args,
            vec!["install", "--noninteractive", "org.gnome.Calculator"]
        );
    }

    #[test]
    fn a_version_is_dropped_because_it_cannot_be_honoured() {
        let commands =
            Flatpak.install_commands(&[PackageSpec::pinned("org.gnome.Calculator", "46.1")]);
        assert_eq!(
            commands[0].args.last().expect("argument"),
            "org.gnome.Calculator"
        );
    }
}
