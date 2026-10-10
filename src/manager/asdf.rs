use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

pub struct Asdf;

impl Manager for Asdf {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "asdf"
    }

    /// `plugin add` is idempotent, so always run; `set` makes the version the one
    /// `asdf current` reads back.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .flat_map(|spec| {
                let name = spec.name.as_str();
                let version = spec.version.as_deref().unwrap_or("latest");
                [
                    Invocation::new("asdf").args(["plugin", "add"]).arg(name),
                    Invocation::new("asdf")
                        .arg("install")
                        .arg(name)
                        .arg(version),
                    Invocation::new("asdf")
                        .args(["set", "--home"])
                        .arg(name)
                        .arg(version),
                ]
            })
            .collect()
    }

    /// `plugin remove`, so an undeclared tool does not linger at any version.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("asdf")
                    .args(["plugin", "remove"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("asdf").arg("latest"),
            unpinned.iter().cloned(),
        )
    }

    /// `current`, not `list`, which prints every installed version.
    fn list_command(&self) -> Invocation {
        Invocation::new("asdf").arg("current")
    }

    /// `Name Version Source Installed`, after a header row. A plugin with no
    /// version set shows `______`; a line without a source column is not a row.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let name = fields.next()?;
                let version = fields.next()?;
                let _source = fields.next()?;
                if name == "Name" || version.starts_with('_') {
                    return None;
                }
                Some(PackageSpec::pinned(name, version))
            })
            .collect()
    }

    /// A tool has one active version and `asdf set` moves it.
    fn supports_pinning(&self) -> bool {
        true
    }

    /// `asdf current` fails before any plugin is added.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `asdf current` with nodejs set and a plugin with nothing set.
    const LIST: &str = "\
Name            Version         Source               Installed
nodejs          26.10.0         /root/.tool-versions true
";
    const NOTHING_SET: &str = "\
Name            Version         Source          Installed
nodejs          ______          ______          
";

    #[test]
    fn the_active_version_is_parsed() {
        assert_eq!(
            Asdf.parse_list(LIST),
            vec![PackageSpec::pinned("nodejs", "26.10.0")]
        );
    }

    #[test]
    fn the_header_row_is_not_a_tool() {
        assert!(Asdf.parse_list(LIST).iter().all(|s| s.name != "Name"));
    }

    #[test]
    fn a_plugin_with_no_version_set_is_not_installed() {
        assert!(
            Asdf.parse_list(NOTHING_SET).is_empty(),
            "`______` is a placeholder, not a version"
        );
    }

    #[test]
    fn a_line_that_is_not_a_table_row_is_not_a_tool() {
        // asdf writes progress such as `Cloning node-build...` to stdout.
        assert!(Asdf.parse_list("Cloning node-build...\n").is_empty());
    }

    #[test]
    fn installing_adds_the_plugin_then_sets_the_version() {
        let commands = Asdf.install_commands(&[PackageSpec::pinned("nodejs", "22.11.0")]);
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].args, vec!["plugin", "add", "nodejs"]);
        assert_eq!(commands[1].args, vec!["install", "nodejs", "22.11.0"]);
        assert_eq!(commands[2].args, vec!["set", "--home", "nodejs", "22.11.0"]);
    }

    #[test]
    fn an_unpinned_tool_takes_latest() {
        let commands = Asdf.install_commands(&[PackageSpec::new("nodejs")]);
        assert_eq!(commands[1].args, vec!["install", "nodejs", "latest"]);
    }
}
