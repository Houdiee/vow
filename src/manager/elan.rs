use crate::exec::Invocation;
use crate::manager::Manager;
use crate::packages::PackageSpec;

/// elan manages Lean toolchains: a name such as `leanprover/lean4:v4.34.1` carries its version.
pub struct Elan;

impl Manager for Elan {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "elan"
    }

    /// One per call: `toolchain install` takes a single toolchain.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                Invocation::new("elan")
                    .args(["toolchain", "install"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("elan")
                    .args(["toolchain", "uninstall"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    /// One toolchain per line, as `leanprover/lean4:v4.34.1`. Declare that
    /// spelling, not an alias like `stable`, which is never reported back.
    fn list_command(&self) -> Invocation {
        Invocation::new("elan").args(["toolchain", "list"])
    }

    /// Only the first field: older elan appends `(default)`.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let name = line.split_whitespace().next()?;
                // `elan toolchain list` says this when there are none.
                if name.is_empty() || name == "no" {
                    return None;
                }
                Some(PackageSpec::new(name))
            })
            .collect()
    }

    /// A toolchain name is its version, so a separate version is a contradiction.
    fn name_selects_version(&self, _name: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `elan toolchain list` with two toolchains installed.
    const LIST: &str = "\
leanprover/lean4:v4.33.0
leanprover/lean4:v4.34.1
";

    #[test]
    fn toolchains_are_the_names() {
        assert_eq!(
            Elan.parse_list(LIST),
            vec![
                PackageSpec::new("leanprover/lean4:v4.33.0"),
                PackageSpec::new("leanprover/lean4:v4.34.1"),
            ]
        );
    }

    #[test]
    fn a_default_marker_would_not_join_the_name() {
        let marked = "leanprover/lean4:v4.34.1 (default)\n";
        assert_eq!(
            Elan.parse_list(marked),
            vec![PackageSpec::new("leanprover/lean4:v4.34.1")]
        );
    }

    #[test]
    fn nothing_comes_back_with_a_version_column() {
        assert!(Elan.parse_list(LIST).iter().all(|s| s.version.is_none()));
    }
}
