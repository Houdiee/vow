use crate::exec::Invocation;
use crate::manager::Manager;
use crate::packages::PackageSpec;

/// pyenv manages Python versions, so a declaration names a version.
pub struct Pyenv;

impl Manager for Pyenv {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "pyenv"
    }

    /// One per call: `pyenv install` builds a Python and takes a single version.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                Invocation::new("pyenv")
                    .args(["install", "--skip-existing"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    /// `--force` so removal does not stop to ask.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("pyenv")
                    .args(["uninstall", "--force"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    /// `--bare`: one version per line, without the active marker or the host's
    /// own `system` Python.
    fn list_command(&self) -> Invocation {
        Invocation::new("pyenv").args(["versions", "--bare"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && *line != "system")
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

    // Captured from `pyenv versions --bare` after `pyenv install 3.12.8`.
    const LIST: &str = "3.12.8\n3.11.9\n";

    #[test]
    fn versions_are_the_names() {
        assert_eq!(
            Pyenv.parse_list(LIST),
            vec![PackageSpec::new("3.12.8"), PackageSpec::new("3.11.9")]
        );
    }

    #[test]
    fn the_hosts_own_python_is_not_managed() {
        let names: Vec<String> = Pyenv
            .parse_list("system\n3.12.8\n")
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["3.12.8"]);
    }
}
