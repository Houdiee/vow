use crate::exec::Invocation;
use crate::manager::{Manager, strip_ansi};
use crate::packages::PackageSpec;

/// zvm manages Zig versions, so a declaration names a version.
pub struct Zvm;

impl Manager for Zvm {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "zvm"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                Invocation::new("zvm")
                    .arg("install")
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("zvm")
                    .arg("uninstall")
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("zvm").arg("ls")
    }

    /// One version per line. zvm paints the active one even with `NO_COLOR`.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        strip_ansi(stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PackageSpec::new)
            .collect()
    }

    /// A version is the name, so a separate version is a contradiction.
    fn name_selects_version(&self, _name: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `zvm ls`: the active 0.14.0 is green even with NO_COLOR.
    const LIST: &str = "0.13.0\n\u{1b}[32m0.14.0\u{1b}[0m\n";

    #[test]
    fn the_active_version_is_not_wrapped_in_escapes() {
        assert_eq!(
            Zvm.parse_list(LIST),
            vec![PackageSpec::new("0.13.0"), PackageSpec::new("0.14.0")]
        );
    }
}
