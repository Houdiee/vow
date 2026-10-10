use crate::exec::Invocation;
use crate::manager::{Manager, parse_two_column};
use crate::packages::PackageSpec;

pub struct Pub;

impl Manager for Pub {
    fn id(&self) -> &'static str {
        "pub"
    }

    fn binary(&self) -> &'static str {
        "dart"
    }

    /// One per call: `activate` takes the version as a separate argument.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                let command = Invocation::new("dart")
                    .args(["pub", "global", "activate"])
                    .arg(spec.name.as_str());
                match &spec.version {
                    Some(version) => command.arg(version.as_str()),
                    None => command,
                }
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("dart")
                    .args(["pub", "global", "deactivate"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    /// Re-activating an unpinned package is how pub upgrades it.
    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        unpinned
            .iter()
            .map(|name| {
                Invocation::new("dart")
                    .args(["pub", "global", "activate"])
                    .arg(name.as_str())
            })
            .collect()
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("dart").args(["pub", "global", "list"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// pub.dev keeps every published version of a package.
    fn supports_pinning(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `dart pub global list` in dart:stable.
    const LIST: &str = "http 1.6.0\nwebdev 3.7.1\n";

    #[test]
    fn name_and_version_are_parsed() {
        assert_eq!(
            Pub.parse_list(LIST),
            vec![
                PackageSpec::pinned("http", "1.6.0"),
                PackageSpec::pinned("webdev", "3.7.1"),
            ]
        );
    }

    #[test]
    fn a_version_is_a_separate_argument() {
        let commands = Pub.install_commands(&[PackageSpec::pinned("http", "1.6.0")]);
        assert_eq!(
            commands[0].args,
            vec!["pub", "global", "activate", "http", "1.6.0"]
        );
    }

    #[test]
    fn each_package_gets_its_own_command() {
        let commands = Pub.install_commands(&[
            PackageSpec::new("http"),
            PackageSpec::pinned("webdev", "3.7.1"),
        ]);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].args, vec!["pub", "global", "activate", "http"]);
    }
}
