use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column, pinned_with};
use crate::packages::PackageSpec;

pub struct Coursier;

impl Manager for Coursier {
    fn id(&self) -> &'static str {
        "coursier"
    }

    fn binary(&self) -> &'static str {
        "cs"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("cs").arg("install"),
            packages.iter().map(|spec| pinned_with(spec, ":")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("cs").arg("uninstall"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("cs").arg("update"),
            unpinned.iter().cloned(),
        )
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("cs").arg("list")
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// Maven Central is append-only: a released artifact is never replaced.
    fn supports_pinning(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `cs list` after `cs install scalafmt`.
    const LIST: &str = "scalafmt 3.11.5\nscalafix 0.14.3\n";

    #[test]
    fn name_and_version_are_parsed() {
        assert_eq!(
            Coursier.parse_list(LIST),
            vec![
                PackageSpec::pinned("scalafmt", "3.11.5"),
                PackageSpec::pinned("scalafix", "0.14.3"),
            ]
        );
    }

    #[test]
    fn the_binary_is_not_the_id() {
        // Declared as `coursier`, run as `cs`.
        assert_eq!(Coursier.id(), "coursier");
        assert_eq!(Coursier.binary(), "cs");
    }
}
