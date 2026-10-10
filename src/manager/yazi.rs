use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

/// yazi's plugin manager, `ya`. Plugins and flavors are managed as one set.
pub struct Yazi;

impl Manager for Yazi {
    fn id(&self) -> &'static str {
        "yazi"
    }

    fn binary(&self) -> &'static str {
        "ya"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("ya").args(["pkg", "add"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("ya").args(["pkg", "delete"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    /// `ya pkg upgrade` moves everything, safe only while nothing is pinned.
    fn upgrade_commands(&self, _unpinned: &[String], pinned: &[String]) -> Vec<Invocation> {
        if !pinned.is_empty() {
            return Vec::new();
        }
        vec![Invocation::new("ya").args(["pkg", "upgrade"])]
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("ya").args(["pkg", "list"])
    }

    /// `Plugins:` and `Flavors:` sections of `owner/repo:name (rev)`; the
    /// revision is a commit, not a version, and is dropped.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter(|line| line.starts_with(char::is_whitespace))
            .filter_map(|line| {
                let name = line.split_whitespace().next()?;
                (!name.is_empty()).then(|| PackageSpec::new(name))
            })
            .collect()
    }

    /// A plugin tracks a branch; `ya pkg add` takes no revision.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `ya pkg list` after adding one plugin and one flavor.
    const LIST: &str = "\
Plugins:
\tyazi-rs/plugins:full-border (6229767)
Flavors:
\tyazi-rs/flavors:dracula (1183892)
";

    #[test]
    fn plugins_and_flavors_are_one_set() {
        assert_eq!(
            Yazi.parse_list(LIST),
            vec![
                PackageSpec::new("yazi-rs/plugins:full-border"),
                PackageSpec::new("yazi-rs/flavors:dracula"),
            ]
        );
    }

    #[test]
    fn the_section_headings_are_not_packages() {
        let names: Vec<String> = Yazi.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 2);
        assert!(!names.contains(&"Plugins:".to_string()));
        assert!(!names.contains(&"Flavors:".to_string()));
    }

    #[test]
    fn an_empty_listing_is_just_the_headings() {
        assert!(Yazi.parse_list("Plugins:\nFlavors:\n").is_empty());
    }

    #[test]
    fn the_revision_is_not_read_as_a_version() {
        assert!(Yazi.parse_list(LIST).iter().all(|s| s.version.is_none()));
    }

    #[test]
    fn packages_are_added_by_their_full_name() {
        let commands = Yazi.install_commands(&[PackageSpec::new("yazi-rs/plugins:full-border")]);
        assert_eq!(
            commands[0].args,
            vec!["pkg", "add", "yazi-rs/plugins:full-border"]
        );
    }
}
