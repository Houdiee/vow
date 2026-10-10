use crate::exec::Invocation;
use crate::manager::Manager;
use crate::packages::PackageSpec;

pub struct Dotnet;

impl Manager for Dotnet {
    fn id(&self) -> &'static str {
        "dotnet"
    }

    // One tool per call: `dotnet tool install` takes a single package.
    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        packages
            .iter()
            .map(|spec| {
                // Without it, a lower version exits 0 and changes nothing.
                let command = Invocation::new("dotnet")
                    .args(["tool", "install", "-g", "--allow-downgrade"])
                    .arg(spec.name.as_str());
                match &spec.version {
                    Some(version) => command.arg("--version").arg(version.as_str()),
                    None => command,
                }
            })
            .collect()
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        installed
            .iter()
            .map(|spec| {
                Invocation::new("dotnet")
                    .args(["tool", "uninstall", "-g"])
                    .arg(spec.name.as_str())
            })
            .collect()
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        Some(
            Invocation::new("dotnet")
                .args(["tool", "search"])
                .arg(query),
        )
    }

    fn list_command(&self) -> Invocation {
        Invocation::new("dotnet").args(["tool", "list", "-g"])
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        // Anchoring on the row of dashes survives header wording and locale.
        let lines: Vec<&str> = stdout.lines().collect();
        let body = match lines
            .iter()
            .position(|line| line.trim_start().starts_with("---"))
        {
            Some(index) => &lines[index + 1..],
            None => lines.get(2..).unwrap_or(&[]),
        };

        body.iter()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let name = parts.next()?;
                Some(match parts.next() {
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
Package Id          Version      Commands
-------------------------------------------
dotnet-ef           8.0.8        dotnet-ef
csharpier           0.29.0       dotnet-csharpier
";

    #[test]
    fn header_and_separator_are_skipped() {
        assert_eq!(
            Dotnet.parse_list(LIST),
            vec![
                PackageSpec::pinned("dotnet-ef", "8.0.8"),
                PackageSpec::pinned("csharpier", "0.29.0")
            ]
        );
    }

    #[test]
    fn missing_separator_falls_back_to_skipping_two_lines() {
        let out = "Package Id Version Commands\ndotnet-ef 8.0.8 dotnet-ef\n";
        assert!(Dotnet.parse_list(out).is_empty());
    }

    #[test]
    fn each_tool_gets_its_own_command() {
        let cmds = Dotnet
            .uninstall_commands(&[PackageSpec::new("dotnet-ef"), PackageSpec::new("csharpier")]);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].args, vec!["tool", "uninstall", "-g", "dotnet-ef"]);
        assert_eq!(cmds[1].args, vec!["tool", "uninstall", "-g", "csharpier"]);
    }

    #[test]
    fn pins_use_the_version_flag_and_permit_a_downgrade() {
        let cmds = Dotnet.install_commands(&[PackageSpec::pinned("dotnet-ef", "8.0.8")]);
        assert_eq!(
            cmds[0].args,
            vec![
                "tool",
                "install",
                "-g",
                "--allow-downgrade",
                "dotnet-ef",
                "--version",
                "8.0.8"
            ]
        );
    }
}
