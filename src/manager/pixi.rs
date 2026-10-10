use crate::exec::Invocation;
use crate::manager::{Manager, batched, pinned_with};
use crate::packages::PackageSpec;

pub struct Pixi;

impl Manager for Pixi {
    fn id(&self) -> &'static str {
        "pixi"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("pixi").args(["global", "install"]),
            packages.iter().map(|spec| pinned_with(spec, "==")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("pixi").args(["global", "uninstall"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("pixi").args(["global", "update"]),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("pixi").arg("search").arg(query))
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("pixi").args(["global", "list"])
    }

    /// `name version`, with exposed binaries beneath as a tree branch -- also at
    /// column zero, so told apart by its glyph.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter(|line| line.starts_with(char::is_alphanumeric))
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let name = fields.next()?;
                let version = fields.next()?;
                Some(PackageSpec::pinned(name, version))
            })
            .collect()
    }

    /// conda-forge keeps published builds.
    fn supports_pinning(&self) -> bool {
        true
    }

    /// `pixi global list` has nothing to report before the first install.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `pixi global list` after `pixi global install jq==1.7.1`.
    const LIST: &str = "\
jq 1.7.1
\u{2514}\u{2500}\u{2500} exposed       jq
ripgrep 14.1.1
\u{2514}\u{2500}\u{2500} exposed       rg
";

    #[test]
    fn packages_are_parsed_and_their_branches_ignored() {
        assert_eq!(
            Pixi.parse_list(LIST),
            vec![
                PackageSpec::pinned("jq", "1.7.1"),
                PackageSpec::pinned("ripgrep", "14.1.1"),
            ]
        );
    }

    #[test]
    fn an_exposed_binary_is_not_a_package() {
        let names: Vec<String> = Pixi.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 2);
        assert!(!names.contains(&"exposed".to_string()));
    }
}
