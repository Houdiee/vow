use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

/// soar installs static binaries and AppImages from its own repositories.
pub struct Soar;

impl Manager for Soar {
    fn id(&self) -> &'static str {
        "soar"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("soar").arg("install"),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("soar").arg("remove"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("soar").arg("update"),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("soar").arg("search").arg(query))
    }

    /// `info`, not `list`, which is the whole catalogue. `--no-color`, since
    /// soar paints even into a pipe.
    fn list_command(&self) -> Invocation {
        Invocation::new("soar").args(["info", "--no-color"])
    }

    /// `name@version:repo (date) (size)` per package, then a summary table.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter(|line| line.starts_with(char::is_alphanumeric))
            .filter_map(|line| {
                let identity = line.split_whitespace().next()?;
                let (name, rest) = identity.split_once('@')?;
                // The repository follows the version, after a colon.
                let version = rest.split(':').next()?;
                if name.is_empty() || version.is_empty() {
                    return None;
                }
                Some(PackageSpec::pinned(name, version))
            })
            .collect()
    }

    /// Installing a version back is unverified against a real soar.
    fn supports_pinning(&self) -> bool {
        false
    }

    /// `soar info` fails until soar has synced and installed something.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `soar info --no-color` after `soar install jq`.
    const LIST: &str = "\
jq@1.8.2:soarpkgs (2026-10-05 05:43:54) (2.17 MiB)
ripgrep@14.1.1:soarpkgs (2026-10-05 05:44:02) (4.20 MiB)

\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}
\u{2502} Summary        \u{2502}
\u{2502} Installed    1 \u{2502}
\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}
";

    #[test]
    fn the_name_and_version_come_out_of_the_identity() {
        assert_eq!(
            Soar.parse_list(LIST),
            vec![
                PackageSpec::pinned("jq", "1.8.2"),
                PackageSpec::pinned("ripgrep", "14.1.1"),
            ]
        );
    }

    #[test]
    fn the_repository_is_not_part_of_the_version() {
        let jq = &Soar.parse_list(LIST)[0];
        assert_eq!(jq.version.as_deref(), Some("1.8.2"));
        assert!(!jq.version.as_deref().unwrap().contains("soarpkgs"));
    }

    #[test]
    fn the_summary_table_is_not_a_package() {
        let names: Vec<String> = Soar.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 2);
        assert!(!names.iter().any(|name| name.contains("Summary")));
    }
}
