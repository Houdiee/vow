use crate::exec::Invocation;
use crate::manager::{Manager, batched, pinned_with};
use crate::packages::PackageSpec;

pub struct Uv;

impl Manager for Uv {
    fn id(&self) -> &'static str {
        "uv"
    }

    /// No `--force` needed, unlike pipx: a different version replaces the old.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("uv").args(["tool", "install"]),
            packages.iter().map(|spec| pinned_with(spec, "==")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("uv").args(["tool", "uninstall"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("uv").args(["tool", "upgrade"]),
            unpinned.iter().cloned(),
        )
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("uv").args(["tool", "list"])
    }

    /// `name vX.Y.Z`, with executables beneath as `- name`.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter(|line| !line.starts_with(char::is_whitespace) && !line.starts_with('-'))
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let name = fields.next()?;
                let version = fields.next()?.strip_prefix('v')?;
                Some(PackageSpec::pinned(name, version))
            })
            .collect()
    }

    fn canonical(&self, name: &str) -> String {
        super::pip::python_name(name)
    }

    /// PyPI keeps published releases.
    fn supports_pinning(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `uv tool list` in ghcr.io/astral-sh/uv:debian.
    const LIST: &str = "\
cowsay v6.1
- cowsay
ruff v0.14.2
- ruff
";

    #[test]
    fn tools_are_parsed_and_their_executables_ignored() {
        assert_eq!(
            Uv.parse_list(LIST),
            vec![
                PackageSpec::pinned("cowsay", "6.1"),
                PackageSpec::pinned("ruff", "0.14.2"),
            ]
        );
    }

    #[test]
    fn an_empty_install_is_not_a_package() {
        assert!(Uv.parse_list("No tools installed\n").is_empty());
    }
}
