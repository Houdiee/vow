use crate::exec::Invocation;
use crate::manager::{Manager, batched};
use crate::packages::PackageSpec;

pub struct Rustup;

impl Manager for Rustup {
    fn stage(&self) -> super::Stage {
        super::Stage::Toolchain
    }

    fn id(&self) -> &'static str {
        "rustup"
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("rustup").args(["toolchain", "install"]),
            packages.iter().map(|spec| spec.name.clone()),
        )
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        batched(
            Invocation::new("rustup").args(["toolchain", "uninstall"]),
            installed.iter().map(|spec| spec.name.clone()),
        )
    }

    fn upgrade_commands(&self, _unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        // `rustup update` cannot leave a toolchain alone.
        Vec::new()
    }

    /// `rustup show` names the host triple, stripped so `1.80.0` reads back as declared.
    fn list_command(&self) -> Invocation {
        Invocation::new("rustup").arg("show")
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        let host = stdout
            .lines()
            .find_map(|line| line.trim().strip_prefix("Default host:"))
            .map(str::trim);

        stdout
            .lines()
            // The toolchains sit under their own heading and its rule.
            .skip_while(|line| line.trim() != "installed toolchains")
            .skip(2)
            .take_while(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let name = line.split_whitespace().next()?;
                // A toolchain for another target keeps its triple.
                let short = host
                    .and_then(|host| name.strip_suffix(host))
                    .map(|stem| stem.trim_end_matches('-'))
                    .unwrap_or(name);
                (!short.is_empty()).then(|| PackageSpec::new(short))
            })
            .collect()
    }

    /// A toolchain for this machine is listed without its host triple, so
    /// `stable-x86_64-unknown-linux-gnu` is `stable`.
    fn canonical(&self, name: &str) -> String {
        let os = match std::env::consts::OS {
            "macos" => "apple-darwin",
            os => os,
        };
        let arch = format!("-{}-", std::env::consts::ARCH);
        match name.find(&arch) {
            Some(at) if name[at..].contains(os) => name[..at].to_string(),
            _ => name.to_string(),
        }
    }

    /// A toolchain name is its version, so a separate version is a contradiction.
    fn name_selects_version(&self, _name: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `rustup show` after installing 1.80.0 and nightly.
    const LIST: &str = "\
Default host: x86_64-unknown-linux-gnu
rustup home:  /usr/local/rustup

installed toolchains
--------------------
nightly-x86_64-unknown-linux-gnu
1.80.0-x86_64-unknown-linux-gnu
1.99.0-x86_64-unknown-linux-gnu (active, default)

active toolchain
----------------
name: 1.99.0-x86_64-unknown-linux-gnu
active because: it's the default toolchain
installed targets:
  x86_64-unknown-linux-gnu
";

    #[test]
    fn the_host_triple_comes_off() {
        assert_eq!(
            Rustup.parse_list(LIST),
            vec![
                PackageSpec::new("nightly"),
                PackageSpec::new("1.80.0"),
                PackageSpec::new("1.99.0"),
            ]
        );
    }

    #[test]
    fn nothing_outside_the_toolchain_section_is_a_toolchain() {
        let names: Vec<String> = Rustup
            .parse_list(LIST)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names.len(), 3);
        for noise in ["Default", "rustup", "name:", "x86_64-unknown-linux-gnu"] {
            assert!(!names.contains(&noise.to_string()), "parsed `{noise}`");
        }
    }

    #[test]
    fn a_toolchain_for_another_target_keeps_its_triple() {
        let cross = "\
Default host: x86_64-unknown-linux-gnu

installed toolchains
--------------------
1.80.0-x86_64-unknown-linux-gnu
1.80.0-aarch64-apple-darwin

";
        assert_eq!(
            Rustup.parse_list(cross),
            vec![
                PackageSpec::new("1.80.0"),
                PackageSpec::new("1.80.0-aarch64-apple-darwin"),
            ]
        );
    }
}
