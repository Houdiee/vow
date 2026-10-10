use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

pub struct Apk;

impl Manager for Apk {
    fn stage(&self) -> super::Stage {
        super::Stage::System
    }

    fn id(&self) -> &'static str {
        "apk"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("apk").with_root().arg("add"),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("apk").with_root().arg("del"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("apk").with_root().args(["upgrade"]),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("apk").args(["search", "-v"]).arg(query))
    }

    /// Bare names, as `apk info -v` joins the version with a `-` names contain too.
    /// Alpine doesn't mark dependencies, so this is everything installed.
    fn list_command(&self) -> Invocation {
        Invocation::new("apk").arg("info")
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PackageSpec::new)
            .collect()
    }

    /// Alpine keeps one version per release branch.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `apk info` in alpine:latest.
    const LIST: &str = "\
alpine-baselayout
alpine-baselayout-data
alpine-keys
ripgrep
";

    #[test]
    fn bare_names_are_parsed() {
        let names: Vec<String> = Apk.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "alpine-baselayout",
                "alpine-baselayout-data",
                "alpine-keys",
                "ripgrep"
            ]
        );
    }

    #[test]
    fn no_version_is_invented() {
        assert!(
            Apk.parse_list(LIST)
                .iter()
                .all(|spec| spec.version.is_none())
        );
    }

    #[test]
    fn writes_need_root() {
        assert!(Apk.install_commands(&[PackageSpec::new("ripgrep")])[0].needs_root);
        assert!(Apk.uninstall_commands(&[PackageSpec::new("ripgrep")])[0].needs_root);
        assert!(!Apk.list_command().needs_root);
    }
}
