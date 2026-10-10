use crate::exec::Invocation;
use crate::manager::{Manager, batched, pinned_with};
use crate::packages::PackageSpec;

pub struct Cargo;

impl Manager for Cargo {
    fn id(&self) -> &'static str {
        "cargo"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("cargo").arg("install"),
            packages.iter().map(|spec| pinned_with(spec, "@")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("cargo").arg("uninstall"),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("cargo").args(["search"]).arg(query))
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        // Reinstalling is how cargo upgrades an installed binary.
        batched(
            Invocation::new("cargo").args(["install", "--force"]),
            unpinned.iter().cloned(),
        )
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("cargo").args(["install", "--list"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        // `name vX.Y.Z:` at column zero, binaries indented beneath.
        stdout
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with(char::is_whitespace))
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let name = parts.next()?;
                let version = parts
                    .next()
                    .map(|v| v.trim_end_matches(':').trim_start_matches('v'))
                    .filter(|v| !v.is_empty());
                Some(match version {
                    Some(version) => PackageSpec::pinned(name, version),
                    None => PackageSpec::new(name),
                })
            })
            .collect()
    }

    fn supports_pinning(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "\
cargo-edit v0.12.2:
    cargo-add
    cargo-rm
ripgrep v14.1.0:
    rg
";

    #[test]
    fn crate_lines_are_parsed_and_binaries_ignored() {
        assert_eq!(
            Cargo.parse_list(LIST),
            vec![
                PackageSpec::pinned("cargo-edit", "0.12.2"),
                PackageSpec::pinned("ripgrep", "14.1.0")
            ]
        );
    }

    #[test]
    fn every_crate_installs_in_one_command_carrying_its_own_version() {
        let cmds = Cargo.install_commands(&[
            PackageSpec::new("bat"),
            PackageSpec::pinned("ripgrep", "14.1.0"),
            PackageSpec::pinned("fd-find", "10.1.0"),
        ]);
        assert_eq!(cmds.len(), 1, "one resolution, not one per pin");
        assert_eq!(
            cmds[0].args,
            vec!["install", "bat", "ripgrep@14.1.0", "fd-find@10.1.0"]
        );
    }

    #[test]
    fn an_upgrade_names_the_unpinned_and_leaves_pins_out() {
        let commands = Cargo.upgrade_commands(
            &["bat".to_string(), "fd-find".to_string()],
            &["ripgrep".to_string()],
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].args,
            vec!["install", "--force", "bat", "fd-find"]
        );
    }
}
