use crate::exec::Invocation;
use crate::manager::{Manager, batched, split_hyphenated};
use crate::packages::PackageSpec;

pub struct Xbps;

impl Manager for Xbps {
    fn stage(&self) -> super::Stage {
        super::Stage::System
    }

    fn id(&self) -> &'static str {
        "xbps"
    }

    fn binary(&self) -> &'static str {
        "xbps-query"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("xbps-install").with_root().args(["-y"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("xbps-remove").with_root().args(["-y"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("xbps-install")
                .with_root()
                .args(["-Syu", "-y"]),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("xbps-query").arg("-Rs").arg(query))
    }

    /// `-m` lists only what was installed on purpose, leaving out dependencies.
    fn list_command(&self) -> Invocation {
        Invocation::new("xbps-query").arg("-m")
    }

    /// Entries are `name-version`, joined by a hyphen that names also contain.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout.lines().filter_map(split_hyphenated).collect()
    }

    /// Void keeps one version per package in its repositories.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `xbps-query -m` in void-glibc-full.
    const LIST: &str = "\
base-container-0.3_3
ripgrep-15.2.0_1
";

    #[test]
    fn manually_installed_packages_are_split_from_their_versions() {
        assert_eq!(
            Xbps.parse_list(LIST),
            vec![
                PackageSpec::pinned("base-container", "0.3_3"),
                PackageSpec::pinned("ripgrep", "15.2.0_1"),
            ]
        );
    }

    #[test]
    fn the_query_binary_is_what_gets_probed() {
        assert_eq!(Xbps.binary(), "xbps-query");
    }
}
