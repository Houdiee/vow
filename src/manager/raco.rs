use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

pub struct Raco;

impl Manager for Raco {
    fn id(&self) -> &'static str {
        "raco"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("raco").args(["pkg", "install", "--auto", "--batch"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("raco").args(["pkg", "remove", "--batch"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        batched(
            Invocation::new("raco").args(["pkg", "update", "--batch"]),
            unpinned.iter().cloned(),
        )
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("raco").args(["pkg", "show"])
    }

    /// A section per scope, each with a header and an `[N auto-installed
    /// packages not shown]` footer: dependencies are already left out.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                // A section heading starts at column zero; entries are indented.
                if !line.starts_with(char::is_whitespace) {
                    return None;
                }
                let name = line.split_whitespace().next()?;
                if name == "Package" || name.starts_with('[') {
                    return None;
                }
                Some(PackageSpec::new(name))
            })
            .collect()
    }

    /// `raco pkg show` reports no version, so a pin could never be checked.
    fn supports_pinning(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `raco pkg show` after installing rackunit-lib.
    const LIST: &str = "\
Installation-wide:
 Package     Checksum                                  Source
 racket-lib  62eb15ddc03bc26a1c0f85c8aff03aa6ef7a518a  catalog racket-lib
 [3 auto-installed packages not shown]
User-specific for installation \"9.3\":
 Package       Checksum                                  Source
 rackunit-lib  47fed14bfd620d43a21fb13ebac42f0f0d8e9f32  catalog rackunit-lib
 [4 auto-installed packages not shown]
";

    #[test]
    fn packages_from_every_scope_are_listed() {
        let names: Vec<String> = Raco
            .parse_list(LIST)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names, vec!["racket-lib", "rackunit-lib"]);
    }

    #[test]
    fn headings_headers_and_footers_are_not_packages() {
        let names: Vec<String> = Raco
            .parse_list(LIST)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        for noise in ["Installation-wide:", "Package", "[3", "User-specific"] {
            assert!(!names.contains(&noise.to_string()), "parsed `{noise}`");
        }
    }
}
