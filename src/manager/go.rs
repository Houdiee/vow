use crate::exec::Invocation;
use crate::manager::Manager;
use crate::packages::PackageSpec;
use std::path::PathBuf;

pub struct Go;

/// Where `go install` puts binaries: `$GOBIN`, else `$GOPATH/bin`, else
/// `$HOME/go/bin`. Read from the environment, so a `GOPATH` set only with
/// `go env -w` is missed.
fn bin_dir() -> PathBuf {
    if let Some(gobin) = std::env::var_os("GOBIN").filter(|value| !value.is_empty()) {
        return PathBuf::from(gobin);
    }
    std::env::var_os("GOPATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("go")))
        .unwrap_or_else(|| PathBuf::from("go"))
        .join("bin")
}

/// The binary `go install` makes: the last element of the import path.
fn binary_name(import_path: &str) -> &str {
    import_path.rsplit('/').next().unwrap_or(import_path)
}

impl Manager for Go {
    fn id(&self) -> &'static str {
        "go"
    }

    /// One `path@version` per call, with `@latest` when unpinned.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                let version = spec.version.as_deref().unwrap_or("latest");
                Invocation::new("go")
                    .arg("install")
                    .arg(format!("{}@{version}", spec.name))
            })
            .collect()
    }

    /// Go has no uninstall; the binary is the installed state.
    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        let dir = bin_dir();
        installed
            .iter()
            .map(|spec| {
                Invocation::new("rm").args(["-f", "--"]).arg(
                    dir.join(binary_name(&spec.name))
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect()
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        unpinned
            .iter()
            .map(|name| {
                Invocation::new("go")
                    .arg("install")
                    .arg(format!("{name}@latest"))
            })
            .collect()
    }

    /// `go version -m <dir>` reads the build metadata stamped into each binary.
    fn list_command(&self) -> Invocation {
        Invocation::new("go")
            .args(["version", "-m"])
            .arg(bin_dir().to_string_lossy().into_owned())
    }

    /// Per binary, a header then tab-indented records: `path` is what was
    /// installed, `mod` its version, `dep` its dependencies. A local build
    /// reports `(devel)` and comes back unpinned.
    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        let mut found = Vec::new();
        let mut path: Option<&str> = None;

        for line in stdout.lines() {
            let Some(record) = line.strip_prefix('\t') else {
                path = None;
                continue;
            };
            let mut fields = record.split('\t');
            match fields.next() {
                Some("path") => path = fields.next(),
                Some("mod") => {
                    let Some(name) = path.take() else { continue };
                    let version = fields.nth(1).filter(|value| *value != "(devel)");
                    found.push(match version {
                        Some(version) => PackageSpec::pinned(name, version),
                        None => PackageSpec::new(name),
                    });
                }
                _ => {}
            }
        }

        found
    }

    /// The module proxy makes a published version immutable.
    fn supports_pinning(&self) -> bool {
        true
    }

    /// `go version -m` fails on an absent bin directory.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `go version -m $(go env GOPATH)/bin` in golang:latest.
    const LIST: &str = "\
/go/bin/goimports: go1.27.1
\tpath\tgolang.org/x/tools/cmd/goimports
\tmod\tgolang.org/x/tools\tv0.24.0\th1:J1shsA93PJUEVaUSaay7UXAyE8aimq3GW0pjlolpa24=
\tdep\tgolang.org/x/mod\tv0.20.0\th1:utOm6MM3R3dnawAiJgn0y+xvuYRsm1RKM/4giyfDgV0=
\tbuild\t-buildmode=exe
/go/bin/hey: go1.27.1
\tpath\tgithub.com/rakyll/hey
\tmod\tgithub.com/rakyll/hey\tv0.1.5\th1:oc3QhpT8ETXcr5xIE2xgWYNSNA/Z52XA20ku9hWCchY=
\tdep\tgolang.org/x/net\tv0.48.0\th1:zyQRTTrjc33Lhh0fBgT/H3oZq9WuvRR5gPC70xpDiQU=
";

    #[test]
    fn each_binary_yields_its_import_path_and_version() {
        assert_eq!(
            Go.parse_list(LIST),
            vec![
                PackageSpec::pinned("golang.org/x/tools/cmd/goimports", "v0.24.0"),
                PackageSpec::pinned("github.com/rakyll/hey", "v0.1.5"),
            ]
        );
    }

    #[test]
    fn dependencies_are_not_installed_packages() {
        let names: Vec<String> = Go.parse_list(LIST).into_iter().map(|s| s.name).collect();
        assert!(!names.contains(&"golang.org/x/mod".to_string()));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn a_locally_built_module_comes_back_unpinned() {
        let devel = "\
/go/bin/mytool: go1.27.1
\tpath\texample.com/mytool
\tmod\texample.com/mytool\t(devel)\t
";
        assert_eq!(
            Go.parse_list(devel),
            vec![PackageSpec::new("example.com/mytool")]
        );
    }

    #[test]
    fn an_unpinned_entry_installs_latest() {
        let commands = Go.install_commands(&[PackageSpec::new("github.com/rakyll/hey")]);
        assert_eq!(
            commands[0].args,
            vec!["install", "github.com/rakyll/hey@latest"]
        );
    }

    #[test]
    fn an_uninstall_removes_the_binary_the_path_names() {
        let commands =
            Go.uninstall_commands(&[PackageSpec::new("golang.org/x/tools/cmd/goimports")]);
        let args = &commands[0].args;
        assert_eq!(args[0], "-f");
        assert!(args[2].ends_with("/goimports"), "removed `{}`", args[2]);
    }
}
