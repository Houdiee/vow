use crate::exec::Invocation;
use crate::manager::Manager;
use crate::manager::node::split_registry_spec;
use crate::packages::PackageSpec;

/// The editors sharing VS Code's extension CLI, differing only in binary.
pub enum Editor {
    Code,
    Codium,
    CodeServer,
}

pub struct VsCode {
    pub editor: Editor,
}

impl VsCode {
    fn command(&self) -> Invocation {
        Invocation::new(self.binary())
    }
}

impl Manager for VsCode {
    fn id(&self) -> &'static str {
        match self.editor {
            Editor::Code => "code",
            Editor::Codium => "codium",
            Editor::CodeServer => "code-server",
        }
    }

    fn binary(&self) -> &'static str {
        self.id()
    }

    /// `--install-extension` repeats, so a set installs in one invocation.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        if packages.is_empty() {
            return Vec::new();
        }
        let mut command = self.command();
        for spec in packages {
            let id = match &spec.version {
                Some(version) => format!("{}@{version}", spec.name),
                None => spec.name.clone(),
            };
            command = command.arg("--install-extension").arg(id);
        }
        vec![command.arg("--force")]
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        if installed.is_empty() {
            return Vec::new();
        }
        let mut command = self.command();
        for spec in installed {
            command = command.arg("--uninstall-extension").arg(spec.name.as_str());
        }
        vec![command]
    }

    fn list_command(&self) -> Invocation {
        self.command()
            .args(["--list-extensions", "--show-versions"])
    }

    /// `publisher.name@version`, one per line. Notices on stdout contain spaces,
    /// which an extension id never does.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty() && !line.contains(char::is_whitespace) && line.contains('@')
            })
            .map(split_registry_spec)
            .filter(|spec| !spec.name.is_empty())
            .collect()
    }

    /// Extension ids ignore case, and the listing prints the publisher's
    /// casing: `GitHub.copilot`.
    fn canonical(&self, name: &str) -> String {
        name.to_ascii_lowercase()
    }

    /// The marketplace keeps older releases of an extension.
    fn supports_pinning(&self) -> bool {
        true
    }

    /// A first run may exit non-zero while writing its configuration.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `code-server --list-extensions --show-versions`, with a first run's notice.
    const LIST: &str = "\
[2026-10-03T13:51:32.798Z] info  Wrote default config file to /home/coder/.config/code-server/config.yaml
esbenp.prettier-vscode@12.4.0
rust-lang.rust-analyzer@0.3.2000
";

    fn code() -> VsCode {
        VsCode {
            editor: Editor::Code,
        }
    }

    #[test]
    fn extensions_are_parsed_with_their_versions() {
        assert_eq!(
            code().parse_list(LIST),
            vec![
                PackageSpec::pinned("esbenp.prettier-vscode", "12.4.0"),
                PackageSpec::pinned("rust-lang.rust-analyzer", "0.3.2000"),
            ]
        );
    }

    #[test]
    fn a_log_line_is_not_an_extension() {
        let names: Vec<String> = code()
            .parse_list(LIST)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|name| !name.contains("info")));
    }

    #[test]
    fn a_whole_set_installs_in_one_invocation() {
        let commands = code().install_commands(&[
            PackageSpec::new("esbenp.prettier-vscode"),
            PackageSpec::pinned("rust-lang.rust-analyzer", "0.3.2000"),
        ]);
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].args,
            vec![
                "--install-extension",
                "esbenp.prettier-vscode",
                "--install-extension",
                "rust-lang.rust-analyzer@0.3.2000",
                "--force",
            ]
        );
    }

    #[test]
    fn each_editor_calls_its_own_binary() {
        assert_eq!(code().binary(), "code");
        assert_eq!(
            VsCode {
                editor: Editor::Codium
            }
            .binary(),
            "codium"
        );
        assert_eq!(
            VsCode {
                editor: Editor::CodeServer
            }
            .binary(),
            "code-server"
        );
    }
}
