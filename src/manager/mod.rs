//! The package managers vow drives, one file each: command shapes, output
//! parser, and tests against output captured from the real tool.

use anyhow::Result;
use std::collections::BTreeMap;

use crate::exec::Invocation;
use crate::packages::PackageSpec;

pub mod apk;
pub mod apt;
pub mod arch;
pub mod asdf;
pub mod bob;
pub mod brew;
pub mod cargo;
pub mod composer;
pub mod coursier;
pub mod dart;
pub mod dnf;
pub mod dotnet;
pub mod elan;
pub mod fisher;
pub mod flatpak;
pub mod gem;
pub mod ghcup;
pub mod go;
pub mod krew;
pub mod luarocks;
pub mod mise;
pub mod node;
pub mod ollama;
pub mod opam;
pub mod pip;
pub mod pipx;
pub mod pixi;
pub mod pyenv;
pub mod raco;
pub mod rustup;
pub mod soar;
pub mod uv;
pub mod volta;
pub mod vscode;
pub mod xbps;
pub mod yazi;
pub mod zvm;
pub mod zypper;

/// When a manager runs in a switch. Each stage finishes before the next, since
/// it can provide what a later one needs (`apt install cargo`, `volta` → `npm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// The operating system's own packages: apt, pacman, brew, ...
    System,
    /// Version managers, which install toolchains: rustup, mise, volta, ...
    Toolchain,
    /// Everything else, which a toolchain usually provides.
    Tools,
}

/// One package manager. Its methods build commands and parse output but never
/// run anything; commands are a `Vec` since some managers take one package per call.
pub trait Manager: Sync {
    fn id(&self) -> &'static str;
    /// The executable probed to decide whether this manager is installed.
    fn binary(&self) -> &'static str {
        self.id()
    }
    fn is_installed(&self) -> bool {
        which::which(self.binary()).is_ok()
    }
    fn stage(&self) -> Stage {
        Stage::Tools
    }
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation>;
    /// Takes the *installed* specs: some managers need the version to remove one.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation>;
    fn list_command(&self) -> Invocation;
    /// Leaves `pinned` alone; empty where vow can drive no safe upgrade.
    fn upgrade_commands(&self, _unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        Vec::new()
    }
    /// May list undeclared packages; the caller filters them.
    fn outdated_command(&self) -> Option<Invocation> {
        None
    }
    fn parse_outdated(&self, _stdout: &str) -> Vec<PackageSpec> {
        Vec::new()
    }
    /// Shown as printed, never parsed.
    fn search_command(&self, _query: &str) -> Option<Invocation> {
        None
    }
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec>;
    /// Whether a failing `list` means nothing is installed yet, as for managers
    /// whose global directory appears with the first install.
    fn empty_until_first_install(&self) -> bool {
        false
    }
    /// Whether an exact version can be installed, and again later; where false,
    /// declaring a version is an error.
    fn supports_pinning(&self) -> bool {
        false
    }
    /// Whether a name selects a version itself, as brew's `node@20` does.
    fn name_selects_version(&self, _name: &str) -> bool {
        false
    }
    /// The name as this manager compares it (pip lists `pyyaml` as `PyYAML`).
    /// Without it vow would install the declared spelling and remove the listed one.
    fn canonical(&self, name: &str) -> String {
        name.to_string()
    }
    /// Why a name can't be declared, where the manager reads it as something
    /// else (npm's `pkg@latest`).
    fn refuses_name(&self, _name: &str) -> Option<&'static str> {
        None
    }
    /// Why an installed package can't be removed, where the manager has no
    /// command for it (volta's runtimes); vow leaves it and warns.
    fn keeps(&self, _name: &str) -> Option<&'static str> {
        None
    }
}

/// In id order, which is the order they are reported in.
static ALL: &[&dyn Manager] = &[
    &apk::Apk,
    &apt::Apt,
    &asdf::Asdf,
    &bob::Bob,
    &brew::Brew,
    &node::Node {
        tool: node::NodeTool::Bun,
    },
    &cargo::Cargo,
    &vscode::VsCode {
        editor: vscode::Editor::Code,
    },
    &vscode::VsCode {
        editor: vscode::Editor::CodeServer,
    },
    &vscode::VsCode {
        editor: vscode::Editor::Codium,
    },
    &composer::Composer,
    &coursier::Coursier,
    &dnf::Dnf,
    &dotnet::Dotnet,
    &elan::Elan,
    &fisher::Fisher,
    &flatpak::Flatpak,
    &gem::Gem,
    &ghcup::Ghcup,
    &go::Go,
    &krew::Krew,
    &luarocks::Luarocks,
    &mise::Mise,
    &node::Node {
        tool: node::NodeTool::Npm,
    },
    &ollama::Ollama,
    &opam::Opam,
    &arch::Arch,
    &pip::Pip,
    &pipx::Pipx,
    &pixi::Pixi,
    &node::Node {
        tool: node::NodeTool::Pnpm,
    },
    &dart::Pub,
    &pyenv::Pyenv,
    &raco::Raco,
    &rustup::Rustup,
    &soar::Soar,
    &uv::Uv,
    &volta::Volta,
    &xbps::Xbps,
    &node::Node {
        tool: node::NodeTool::Yarn,
    },
    &yazi::Yazi,
    &zvm::Zvm,
    &zypper::Zypper,
];

pub fn all() -> impl Iterator<Item = &'static dyn Manager> {
    ALL.iter().copied()
}

pub fn get(id: &str) -> Option<&'static dyn Manager> {
    all().find(|manager| manager.id() == id)
}

/// What a manager has installed, by its [`Manager::canonical`] name.
pub fn installed(manager: &dyn Manager) -> Result<BTreeMap<String, PackageSpec>> {
    let stdout = match manager.list_command().capture() {
        Ok(stdout) => stdout,
        Err(_) if manager.empty_until_first_install() => String::new(),
        Err(error) => {
            return Err(error.context(format!(
                "could not list installed packages for `{}`",
                manager.id()
            )));
        }
    };
    Ok(manager
        .parse_list(&stdout)
        .into_iter()
        .map(|spec| (manager.canonical(&spec.name), spec))
        .collect())
}

/// One command covering every package, or nothing when there are none.
pub(crate) fn batched<I, S>(command: Invocation, packages: I) -> Vec<Invocation>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut peekable = packages.into_iter().peekable();
    if peekable.peek().is_none() {
        return Vec::new();
    }
    vec![command.args(peekable)]
}

/// `ripgrep@14.1.0`, `ripgrep==14.1.0`, ...: a pin with this manager's separator.
pub(crate) fn pinned_with(spec: &PackageSpec, separator: &str) -> String {
    match &spec.version {
        Some(version) => format!("{}{separator}{version}", spec.name),
        None => spec.name.clone(),
    }
}

/// Split `name-version` at the last hyphen before a digit, since names have
/// hyphens too (`alpine-baselayout-3.7.2-r1`); matches `xbps-uhelper getpkgname`.
pub(crate) fn split_hyphenated(entry: &str) -> Option<PackageSpec> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    let mut boundary = entry.char_indices().filter(|(index, character)| {
        *character == '-' && entry[index + 1..].starts_with(|next: char| next.is_ascii_digit())
    });
    match boundary.next_back() {
        Some((at, _)) => Some(PackageSpec::pinned(&entry[..at], &entry[at + 1..])),
        None => Some(PackageSpec::new(entry)),
    }
}

/// For the few managers that colour their output even into a pipe.
pub(crate) fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text.chars();
    while let Some(character) = rest.next() {
        if character != '\u{1b}' {
            out.push(character);
            continue;
        }
        // A CSI sequence runs from `[` to its final byte in `@`..`~`.
        if rest.next() != Some('[') {
            continue;
        }
        for byte in rest.by_ref() {
            if ('@'..='~').contains(&byte) {
                break;
            }
        }
    }
    out
}

/// Parse `name version` lines, tolerating extra trailing columns and blanks.
pub(crate) fn parse_two_column(stdout: &str) -> Vec<PackageSpec> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            Some(match fields.next() {
                Some(version) => PackageSpec::pinned(name, version),
                None => PackageSpec::new(name),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_spelling_matches_the_listed_one() {
        let same = |id: &str, declared: &str, listed: &str| {
            let manager = get(id).expect("registered");
            assert_eq!(
                manager.canonical(declared),
                manager.canonical(listed),
                "{id}: `{declared}` is `{listed}`"
            );
        };
        same("pip", "pyyaml", "PyYAML");
        same("pip", "typing-extensions", "typing_extensions");
        same("pip", "requests[socks]", "requests");
        same("pipx", "Zope.Interface", "zope-interface");
        same("code", "github.copilot", "GitHub.copilot");
        same("brew", "user/tap/pkg", "pkg");
        same(
            "flatpak",
            "org.gnome.Calculator//stable",
            "org.gnome.Calculator",
        );
        let host = format!(
            "stable-{}-unknown-{}-gnu",
            std::env::consts::ARCH,
            std::env::consts::OS
        );
        if std::env::consts::OS == "linux" {
            same("rustup", &host, "stable");
        }
        // A toolchain for another machine keeps its triple.
        let rustup = get("rustup").expect("registered");
        assert_ne!(
            rustup.canonical("stable-riscv64gc-unknown-none-elf"),
            "stable"
        );
    }

    #[test]
    fn system_managers_run_first_then_toolchains_then_the_rest() {
        let stage = |id: &str| get(id).expect("registered").stage();
        for id in ["apk", "apt", "brew", "dnf", "pacman", "xbps", "zypper"] {
            assert_eq!(stage(id), Stage::System, "{id}");
        }
        for id in ["asdf", "mise", "rustup", "volta", "pyenv", "ghcup"] {
            assert_eq!(stage(id), Stage::Toolchain, "{id}");
        }
        for id in ["cargo", "npm", "pnpm", "pipx", "flatpak"] {
            assert_eq!(stage(id), Stage::Tools, "{id}");
        }
        assert!(Stage::System < Stage::Toolchain && Stage::Toolchain < Stage::Tools);
    }

    #[test]
    fn the_registry_is_sorted_and_unique() {
        let ids: Vec<&str> = all().map(|manager| manager.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, ids);
    }

    #[test]
    fn a_version_is_accepted_only_where_it_can_be_held() {
        for id in ["cargo", "composer", "dotnet", "npm", "pipx", "pnpm", "bun"] {
            assert!(
                get(id).expect("registered").supports_pinning(),
                "`{id}` should accept versions"
            );
        }
        for id in [
            "apk", "apt", "brew", "dnf", "flatpak", "gem", "luarocks", "pacman", "xbps",
        ] {
            assert!(
                !get(id).expect("registered").supports_pinning(),
                "`{id}` should reject versions"
            );
        }
    }

    #[test]
    fn a_listing_that_fails_before_the_first_install_is_empty() {
        struct Fresh;
        impl Manager for Fresh {
            fn id(&self) -> &'static str {
                "fresh"
            }
            fn install_commands(&self, _: &[PackageSpec]) -> Vec<Invocation> {
                Vec::new()
            }
            fn uninstall_commands(&self, _: &[PackageSpec]) -> Vec<Invocation> {
                Vec::new()
            }
            fn list_command(&self) -> Invocation {
                Invocation::new("false")
            }
            fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
                parse_two_column(stdout)
            }
            fn empty_until_first_install(&self) -> bool {
                true
            }
        }
        assert!(installed(&Fresh).expect("not a fault").is_empty());
    }

    #[test]
    fn colour_sequences_come_off() {
        assert_eq!(strip_ansi("\u{1b}[32m0.14.0\u{1b}[0m"), "0.14.0");
        assert_eq!(strip_ansi("jq 1.8.2\n"), "jq 1.8.2\n");
    }

    #[test]
    fn one_command_covers_every_package_or_none() {
        assert!(batched(Invocation::new("x").arg("add"), Vec::<String>::new()).is_empty());
        let commands = batched(Invocation::new("x").arg("add"), ["a", "b"]);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].args, vec!["add", "a", "b"]);
    }

    #[test]
    fn a_pin_is_spelled_however_the_manager_wants() {
        let spec = PackageSpec::pinned("ripgrep", "14.1.0");
        assert_eq!(pinned_with(&spec, "@"), "ripgrep@14.1.0");
        assert_eq!(pinned_with(&spec, "=="), "ripgrep==14.1.0");
        assert_eq!(pinned_with(&PackageSpec::new("ripgrep"), "@"), "ripgrep");
    }

    #[test]
    fn a_hyphenated_entry_splits_at_the_last_hyphen_before_a_digit() {
        for (entry, name, version) in [
            ("ripgrep-15.2.0_1", "ripgrep", "15.2.0_1"),
            (
                "alpine-baselayout-3.7.2-r1",
                "alpine-baselayout",
                "3.7.2-r1",
            ),
            ("wine-32bit-9.0_1", "wine-32bit", "9.0_1"),
            ("foo-2-1.0_1", "foo-2", "1.0_1"),
            ("gtk+-2.24.33_1", "gtk+", "2.24.33_1"),
            ("qt5-5.15.11_1", "qt5", "5.15.11_1"),
        ] {
            assert_eq!(
                split_hyphenated(entry),
                Some(PackageSpec::pinned(name, version)),
                "`{entry}`"
            );
        }
        assert_eq!(
            split_hyphenated("alpine-keys"),
            Some(PackageSpec::new("alpine-keys"))
        );
        assert_eq!(split_hyphenated("   "), None);
    }
}

#[cfg(test)]
mod container;
