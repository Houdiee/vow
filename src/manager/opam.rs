use crate::exec::Invocation;
use crate::manager::{Manager, batched, parse_two_column, pinned_with};
use crate::packages::PackageSpec;

pub struct Opam;

impl Manager for Opam {
    fn id(&self) -> &'static str {
        "opam"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("opam").args(["install", "--yes"]),
            packages.iter().map(|spec| pinned_with(spec, ".")),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("opam").args(["remove", "--yes"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("opam").args(["upgrade", "--yes"]),
            unpinned.iter().cloned(),
        )
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(Invocation::new("opam").arg("search").arg(query))
    }

    /// `--installed-roots` leaves out dependencies; `--columns` fixes the shape.
    fn list_command(&self) -> Invocation {
        Invocation::new("opam").args([
            "list",
            "--installed-roots",
            "--short",
            "--columns=name,version",
        ])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        parse_two_column(stdout)
    }

    /// The opam repository keeps every published release of a package.
    fn supports_pinning(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `opam list --installed-roots --short --columns=name,version`.
    const LIST: &str = "\
ocaml-base-compiler 5.5.1
ocamlfind           1.9.9~preview
opam-depext         1.2.3
";

    #[test]
    fn aligned_columns_are_parsed() {
        assert_eq!(
            Opam.parse_list(LIST),
            vec![
                PackageSpec::pinned("ocaml-base-compiler", "5.5.1"),
                PackageSpec::pinned("ocamlfind", "1.9.9~preview"),
                PackageSpec::pinned("opam-depext", "1.2.3"),
            ]
        );
    }

    #[test]
    fn pins_use_a_dot() {
        let commands = Opam.install_commands(&[PackageSpec::pinned("ocamlfind", "1.9.9")]);
        assert_eq!(
            commands[0].args,
            vec!["install", "--yes", "ocamlfind.1.9.9"]
        );
    }
}
