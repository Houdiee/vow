use crate::exec::Invocation;
use crate::manager::Manager;
use crate::packages::PackageSpec;

/// ollama manages local models: a named set, pulled and removed by name.
pub struct Ollama;

impl Manager for Ollama {
    fn id(&self) -> &'static str {
        "ollama"
    }

    /// One per call: `ollama pull` takes a single model.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                Invocation::new("ollama")
                    .arg("pull")
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| Invocation::new("ollama").arg("rm").arg(spec.name.as_str()))
            .collect()
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("ollama").arg("list")
    }

    /// `NAME ID SIZE MODIFIED` after a header; the id is a digest, not a version.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .filter_map(|line| {
                let name = line.split_whitespace().next()?;
                if name.is_empty() || name == "NAME" {
                    return None;
                }
                Some(PackageSpec::new(name))
            })
            .collect()
    }

    /// The tag is part of the name (`smollm:135m`), so a separate version is a
    /// contradiction. Declare the tag: bare `smollm` never matches the listing.
    fn name_selects_version(&self, name: &str) -> bool {
        name.contains(':')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `ollama list` after `ollama pull smollm:135m`.
    const LIST: &str = "\
NAME           ID              SIZE     MODIFIED               
smollm:135m    b0b2a4617438    91 MB    Less than a second ago
qwen2.5:0.5b   a8b0c5157008    397 MB   2 days ago
";

    #[test]
    fn model_names_keep_their_tag() {
        assert_eq!(
            Ollama.parse_list(LIST),
            vec![
                PackageSpec::new("smollm:135m"),
                PackageSpec::new("qwen2.5:0.5b"),
            ]
        );
    }

    #[test]
    fn the_header_row_is_not_a_model() {
        assert!(Ollama.parse_list(LIST).iter().all(|s| s.name != "NAME"));
    }

    #[test]
    fn the_digest_is_not_read_as_a_version() {
        assert!(Ollama.parse_list(LIST).iter().all(|s| s.version.is_none()));
    }

    #[test]
    fn a_tagged_name_already_selects_its_version() {
        assert!(Ollama.name_selects_version("smollm:135m"));
        assert!(!Ollama.name_selects_version("smollm"));
    }
}
