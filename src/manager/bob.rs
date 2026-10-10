use crate::exec::Invocation;
use crate::manager::Manager;
use crate::packages::PackageSpec;

/// bob installs Neovim versions, so a declaration names a version.
pub struct Bob;

impl Manager for Bob {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "bob"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                Invocation::new("bob")
                    .arg("install")
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("bob")
                    .arg("uninstall")
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("bob").arg("list")
    }

    /// A box-drawing table; only `Version` is read, avoiding the colour on `Status`.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let mut cells = line.split('\u{2502}');
                // Skip what precedes the opening `│`.
                cells.next()?;
                let version = cells.next()?.trim();
                if version.is_empty() || version == "Version" {
                    return None;
                }
                Some(PackageSpec::new(version))
            })
            .collect()
    }

    /// A version *is* the name, so a separate version is a contradiction.
    fn name_selects_version(&self, _name: &str) -> bool {
        true
    }

    /// `bob list` fails before anything has been installed.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `bob list` with two versions installed.
    const LIST: &str = "\
\u{250c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}
\u{2502}  Version  \u{2502}  Status     \u{2502}
\u{251c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{253c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2524}
\u{2502}  v0.12.5  \u{2502}  \u{1b}[33mInstalled\u{1b}[0m  \u{2502}
\u{2502}  v0.10.4  \u{2502}  \u{1b}[33mInstalled\u{1b}[0m  \u{2502}
\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2534}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}
";

    #[test]
    fn the_version_column_is_the_name() {
        assert_eq!(
            Bob.parse_list(LIST),
            vec![PackageSpec::new("v0.12.5"), PackageSpec::new("v0.10.4")]
        );
    }

    #[test]
    fn the_header_and_rules_are_not_versions() {
        let names: Vec<String> = Bob.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 2);
        assert!(!names.contains(&"Version".to_string()));
    }

    #[test]
    fn the_coloured_status_cell_is_never_read() {
        assert!(
            Bob.parse_list(LIST)
                .iter()
                .all(|spec| !spec.name.contains('\u{1b}') && !spec.name.contains("Installed"))
        );
    }
}
