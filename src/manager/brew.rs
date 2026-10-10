use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column};
use crate::packages::PackageSpec;

pub struct Brew;

impl Manager for Brew {
    fn stage(&self) -> super::Stage {
        super::Stage::System
    }

    fn id(&self) -> &'static str {
        "brew"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("brew").arg("install"),
            packages.iter().map(|s| s.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("brew").arg("uninstall"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    /// The caller leaves out pins, including names like `node@20`.
    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("brew").arg("upgrade"),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("brew").args(["search"]).arg(query))
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("brew").args(["list", "--formula", "--versions"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// A formula from a tap, `user/tap/pkg`, is listed by its own name.
    fn canonical(&self, name: &str) -> String {
        name.rsplit('/').next().unwrap_or(name).to_string()
    }

    /// Versioned formulae such as `node@20` are distinct packages.
    fn name_selects_version(&self, name: &str) -> bool {
        match name.split_once('@') {
            Some((head, tail)) => {
                !head.is_empty() && tail.starts_with(|character: char| character.is_ascii_digit())
            }
            None => false,
        }
    }

    /// Homebrew cannot install an old version; a versioned formula can.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_output_is_parsed() {
        let out = "git 2.46.0\nripgrep 14.1.0\nvim 9.1.0750\n";
        assert_eq!(
            Brew.parse_list(out),
            vec![
                PackageSpec::pinned("git", "2.46.0"),
                PackageSpec::pinned("ripgrep", "14.1.0"),
                PackageSpec::pinned("vim", "9.1.0750"),
            ]
        );
    }

    #[test]
    fn a_versioned_formula_keeps_its_name_intact() {
        assert_eq!(
            Brew.parse_list("node@20 20.11.0\n"),
            vec![PackageSpec::pinned("node@20", "20.11.0")]
        );
    }

    #[test]
    fn a_formula_with_several_versions_keeps_the_first() {
        assert_eq!(
            Brew.parse_list("openssl 3.3.1 3.2.0\n"),
            vec![PackageSpec::pinned("openssl", "3.3.1")]
        );
    }

    #[test]
    fn versioned_formulae_are_recognised() {
        for name in ["node@20", "python@3.12", "postgresql@16"] {
            assert!(
                Brew.name_selects_version(name),
                "`{name}` should select a version"
            );
        }
        for name in ["ripgrep", "vim"] {
            assert!(!Brew.name_selects_version(name));
        }
    }

    #[test]
    fn an_at_not_followed_by_a_digit_is_not_a_version() {
        assert!(!Brew.name_selects_version("@scope/pkg"));
        assert!(!Brew.name_selects_version("foo@bar"));
    }
}
