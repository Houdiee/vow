use crate::exec::Invocation;
use crate::manager::Manager;
use crate::manager::node::split_registry_spec;
use crate::packages::PackageSpec;

/// Volta manages Node runtimes and global packages as one set.
pub struct Volta;

impl Manager for Volta {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "volta"
    }

    /// One tool per call: `volta install` takes a single `tool@version`.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                let name = match &spec.version {
                    Some(version) => format!("{}@{version}", spec.name),
                    None => spec.name.clone(),
                };
                Invocation::new("volta").arg("install").arg(name)
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("volta")
                    .arg("uninstall")
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("volta").args(["list", "all"])
    }

    /// `runtime node@22.23.3 (default)` and
    /// `package typescript@7.0.2 / tsc / node@22.23.3 npm@built-in (default)`;
    /// only the second column is read.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let kind = fields.next()?;
                if kind != "runtime" && kind != "package" {
                    return None;
                }
                let spec = split_registry_spec(fields.next()?);
                (!spec.name.is_empty()).then_some(spec)
            })
            .collect()
    }

    /// Node releases and the npm registry keep published versions.
    fn supports_pinning(&self) -> bool {
        true
    }

    /// `volta list all` says nothing before the first install.
    fn empty_until_first_install(&self) -> bool {
        true
    }

    /// `volta uninstall node` answers "not supported yet".
    fn keeps(&self, name: &str) -> Option<&'static str> {
        (name == "node").then_some("volta cannot uninstall a Node runtime")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `volta list all` after installing node@22 and typescript.
    const LIST: &str = "\
runtime node@22.23.3 (default)
package typescript@7.0.2 / tsc / node@22.23.3 npm@built-in (default)
";

    #[test]
    fn runtimes_and_packages_are_both_listed() {
        assert_eq!(
            Volta.parse_list(LIST),
            vec![
                PackageSpec::pinned("node", "22.23.3"),
                PackageSpec::pinned("typescript", "7.0.2"),
            ]
        );
    }

    #[test]
    fn the_trailing_columns_are_not_packages() {
        let names: Vec<String> = Volta.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 2);
        assert!(!names.contains(&"tsc".to_string()));
    }

    #[test]
    fn a_runtime_is_kept_and_a_package_is_not() {
        assert!(Volta.keeps("node").is_some());
        assert!(Volta.keeps("typescript").is_none());
    }

    #[test]
    fn a_pin_is_attached_with_an_at_sign() {
        let commands = Volta.install_commands(&[PackageSpec::pinned("node", "22.23.3")]);
        assert_eq!(commands[0].args, vec!["install", "node@22.23.3"]);
    }
}
