use crate::exec::Invocation;
use crate::manager::{Manager, batched, pinned_with};
use crate::packages::PackageSpec;

/// The npm-registry clients, differing only in subcommands and listing format.
pub enum NodeTool {
    Bun,
    Npm,
    Pnpm,
    Yarn,
}

pub struct Node {
    pub tool: NodeTool,
}

impl Manager for Node {
    fn id(&self) -> &'static str {
        match self.tool {
            NodeTool::Bun => "bun",
            NodeTool::Npm => "npm",
            NodeTool::Pnpm => "pnpm",
            NodeTool::Yarn => "yarn",
        }
    }

    fn install_commands(&self, packages: &[PackageSpec]) -> Vec<Invocation> {
        if packages.is_empty() {
            return Vec::new();
        }
        let specs = packages.iter().map(|spec| pinned_with(spec, "@"));
        vec![match self.tool {
            NodeTool::Bun => Invocation::new("bun").args(["add", "-g"]).args(specs),
            NodeTool::Npm => Invocation::new("npm").args(["install", "-g"]).args(specs),
            // `pnpm install -g` does not add a global package; `pnpm add -g` does.
            NodeTool::Pnpm => Invocation::new("pnpm").args(["add", "-g"]).args(specs),
            NodeTool::Yarn => Invocation::new("yarn").args(["global", "add"]).args(specs),
        }]
    }

    fn uninstall_commands(&self, installed: &[PackageSpec]) -> Vec<Invocation> {
        if installed.is_empty() {
            return Vec::new();
        }
        let names = installed.iter().map(|spec| spec.name.clone());
        vec![match self.tool {
            NodeTool::Bun => Invocation::new("bun").args(["remove", "-g"]).args(names),
            NodeTool::Npm => Invocation::new("npm").args(["uninstall", "-g"]).args(names),
            NodeTool::Pnpm => Invocation::new("pnpm").args(["remove", "-g"]).args(names),
            NodeTool::Yarn => Invocation::new("yarn")
                .args(["global", "remove"])
                .args(names),
        }]
    }

    fn upgrade_commands(&self, unpinned: &[String], _pinned: &[String]) -> Vec<Invocation> {
        match self.tool {
            NodeTool::Npm => batched(
                Invocation::new("npm").args(["update", "-g"]),
                unpinned.iter().cloned(),
            ),
            // Unverified against the real tools; a wrong upgrade is worse than none.
            NodeTool::Bun | NodeTool::Pnpm | NodeTool::Yarn => Vec::new(),
        }
    }

    fn outdated_command(&self) -> Option<Invocation> {
        match self.tool {
            NodeTool::Npm => Some(Invocation::new("npm").args(["outdated", "-g"])),
            NodeTool::Bun | NodeTool::Pnpm | NodeTool::Yarn => None,
        }
    }

    /// `Package Current Wanted Latest Location Depended by`, after a header row.
    fn parse_outdated(&self, stdout: &str) -> Vec<PackageSpec> {
        stdout
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let [name, _current, _wanted, latest, ..] = fields.as_slice() else {
                    return None;
                };
                Some(PackageSpec::pinned(*name, *latest))
            })
            .collect()
    }

    fn search_command(&self, query: &str) -> Option<Invocation> {
        match self.tool {
            NodeTool::Npm => Some(Invocation::new("npm").arg("search").arg(query)),
            // Neither has a search of its own.
            NodeTool::Bun | NodeTool::Pnpm | NodeTool::Yarn => None,
        }
    }

    fn list_command(&self) -> Invocation {
        match self.tool {
            NodeTool::Bun => Invocation::new("bun").args(["pm", "ls", "-g"]),
            NodeTool::Npm => Invocation::new("npm").args(["list", "-g", "--depth=0"]),
            NodeTool::Pnpm => Invocation::new("pnpm").args(["list", "-g", "--depth=0"]),
            NodeTool::Yarn => Invocation::new("yarn").args(["global", "list"]),
        }
    }

    fn parse_list(&self, stdout: &str) -> Vec<PackageSpec> {
        match self.tool {
            NodeTool::Bun | NodeTool::Npm => parse_registry_tree(stdout),
            NodeTool::Pnpm => parse_pnpm(stdout),
            NodeTool::Yarn => parse_yarn(stdout),
        }
    }

    fn supports_pinning(&self) -> bool {
        true
    }

    /// `pkg@latest` would install, then list as `pkg` and be removed as
    /// undeclared. A scope's `@` comes first, so only a later one is a tag.
    fn refuses_name(&self, name: &str) -> Option<&'static str> {
        name.chars()
            .skip(1)
            .any(|character| character == '@')
            .then_some("put an exact version after a space, or none to track the latest")
    }

    /// `npm list -g` exits 254 with ENOENT until the global prefix exists;
    /// bun, pnpm and yarn are treated the same way.
    fn empty_until_first_install(&self) -> bool {
        true
    }
}

/// Split `name@version` at the last `@`; a leading one is a scope.
pub(crate) fn split_registry_spec(text: &str) -> PackageSpec {
    match text.rfind('@') {
        Some(at) if at > 0 => {
            let (name, rest) = text.split_at(at);
            let version = rest[1..].trim();
            if version.is_empty() {
                PackageSpec::new(name)
            } else {
                PackageSpec::pinned(name, version)
            }
        }
        _ => PackageSpec::new(text),
    }
}

/// The global prefix, then a box-drawing tree of `name@version`, as npm and
/// bun print it.
pub(crate) fn parse_registry_tree(stdout: &str) -> Vec<PackageSpec> {
    stdout
        .lines()
        .skip(1) // the global prefix path
        .filter_map(|line| {
            let start = line
                .char_indices()
                .find(|(_, character)| {
                    character.is_alphanumeric() || matches!(character, '@' | '(')
                })
                .map(|(index, _)| index)?;
            // npm prints `(empty)` under a prefix that holds nothing.
            if line[start..].starts_with('(') {
                return None;
            }
            // A linked package goes on to name where it links: `name@1.0 -> ./path`.
            let entry = line[start..].split_whitespace().next()?;
            let spec = split_registry_spec(entry);
            if spec.name.is_empty() {
                None
            } else {
                Some(spec)
            }
        })
        .collect()
}

/// A legend, the store path, then `dependencies:` -- as `name version`, or since
/// pnpm 12 as npm's `name@version` tree.
fn parse_pnpm(stdout: &str) -> Vec<PackageSpec> {
    stdout
        .lines()
        .skip_while(|line| !line.contains("dependencies:"))
        .skip(1)
        .filter_map(|line| {
            let start = line
                .char_indices()
                .find(|(_, character)| character.is_alphanumeric() || *character == '@')
                .map(|(index, _)| index)?;
            let entry = line[start..].trim();
            match entry.split_once(char::is_whitespace) {
                Some((name, version)) => Some(PackageSpec::pinned(name, version.trim())),
                None => {
                    let spec = split_registry_spec(entry);
                    if spec.name.is_empty() {
                        None
                    } else {
                        Some(spec)
                    }
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `npm outdated -g` in node:slim.
    const OUTDATED: &str = "\
Package  Current  Wanted  Latest  Location             Depended by
is-odd     2.0.0   3.0.1   3.0.1  node_modules/is-odd  global
npm      11.19.1  12.2.0  12.2.0  node_modules/npm     global
";

    const NPM_LIST: &str = "\
/usr/lib
├── @anthropic-ai/claude-code@1.0.0
├── chromedriver@147.0.0 -> ./../../node-tools/node_modules/chromedriver
├── corepack@0.24.0
└── typescript@5.3.3
";

    // Captured from `pnpm list -g --depth=0` under pnpm 12.
    const PNPM_LIST: &str = "\
Legend: production dependency, optional only, dev only

/root/.pnpm/global/v11 (PRIVATE)
│
│   dependencies:
└── is-odd@3.0.1
";

    #[test]
    fn npm_scoped_packages_survive_parsing() {
        let npm = Node {
            tool: NodeTool::Npm,
        };
        assert_eq!(
            npm.parse_list(NPM_LIST),
            vec![
                PackageSpec::pinned("@anthropic-ai/claude-code", "1.0.0"),
                PackageSpec::pinned("chromedriver", "147.0.0"),
                PackageSpec::pinned("corepack", "0.24.0"),
                PackageSpec::pinned("typescript", "5.3.3"),
            ]
        );
    }

    #[test]
    fn an_empty_prefix_is_not_a_package() {
        let npm = Node {
            tool: NodeTool::Npm,
        };
        assert!(npm.parse_list("/tmp/prefix/lib\n└── (empty)\n").is_empty());
    }

    #[test]
    fn pnpm_saying_it_has_nothing_is_not_a_package() {
        // Captured from `pnpm list -g --depth=0` with an empty global store.
        let pnpm = Node {
            tool: NodeTool::Pnpm,
        };
        assert!(pnpm.parse_list("No global packages found\n").is_empty());
    }

    #[test]
    fn pnpm_reads_the_dependencies_section() {
        let pnpm = Node {
            tool: NodeTool::Pnpm,
        };
        assert_eq!(
            pnpm.parse_list(PNPM_LIST),
            vec![PackageSpec::pinned("is-odd", "3.0.1")]
        );
    }

    #[test]
    fn the_older_pnpm_spelling_still_parses() {
        let pnpm = Node {
            tool: NodeTool::Pnpm,
        };
        let old = "/root/.pnpm\n\ndependencies:\ntypescript 5.3.3\n";
        assert_eq!(
            pnpm.parse_list(old),
            vec![PackageSpec::pinned("typescript", "5.3.3")]
        );
    }

    #[test]
    fn the_legend_and_store_path_are_not_packages() {
        let pnpm = Node {
            tool: NodeTool::Pnpm,
        };
        let names: Vec<String> = pnpm
            .parse_list(PNPM_LIST)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names, vec!["is-odd"]);
    }

    #[test]
    fn a_pin_becomes_registry_syntax_not_declaration_syntax() {
        let npm = Node {
            tool: NodeTool::Npm,
        };
        let commands = npm.install_commands(&[PackageSpec::pinned("typescript", "5.3.3")]);
        assert_eq!(commands[0].args, vec!["install", "-g", "typescript@5.3.3"]);
    }

    #[test]
    fn scoped_pins_keep_the_scope() {
        let npm = Node {
            tool: NodeTool::Npm,
        };
        let commands = npm.install_commands(&[PackageSpec::pinned("@scope/pkg", "1.0.0")]);
        assert_eq!(commands[0].args, vec!["install", "-g", "@scope/pkg@1.0.0"]);
    }

    #[test]
    fn the_latest_column_is_the_one_that_matters() {
        let npm = Node {
            tool: NodeTool::Npm,
        };
        assert_eq!(
            npm.parse_outdated(OUTDATED),
            vec![
                PackageSpec::pinned("is-odd", "3.0.1"),
                PackageSpec::pinned("npm", "12.2.0"),
            ]
        );
    }

    #[test]
    fn npm_upgrades_the_unpinned_and_pnpm_is_left_alone() {
        let npm = Node {
            tool: NodeTool::Npm,
        };
        assert_eq!(
            npm.upgrade_commands(&["is-odd".to_string()], &["typescript".to_string()])[0].args,
            vec!["update", "-g", "is-odd"]
        );
        let pnpm = Node {
            tool: NodeTool::Pnpm,
        };
        assert!(
            pnpm.upgrade_commands(&["is-odd".to_string()], &[])
                .is_empty()
        );
    }
}

/// An `info` line per package, binaries beneath; a package with no binary is never listed.
pub(crate) fn parse_yarn(stdout: &str) -> Vec<PackageSpec> {
    stdout
        .lines()
        .filter_map(|line| {
            let quoted = line.trim().strip_prefix("info ")?.strip_prefix('"')?;
            let (identity, _) = quoted.split_once('"')?;
            let spec = split_registry_spec(identity);
            (!spec.name.is_empty()).then_some(spec)
        })
        .collect()
}

#[cfg(test)]
mod yarn_tests {
    use super::*;

    // Captured from `yarn global list` after adding prettier; the banner must not parse.
    const LIST: &str = "\
yarn global v1.22.22
info \"prettier@3.9.9\" has binaries:
   - prettier
info \"@scope/tool@2.1.0\" has binaries:
   - tool
Done in 0.03s.
";

    fn yarn() -> Node {
        Node {
            tool: NodeTool::Yarn,
        }
    }

    #[test]
    fn the_info_lines_are_the_packages() {
        assert_eq!(
            yarn().parse_list(LIST),
            vec![
                PackageSpec::pinned("prettier", "3.9.9"),
                PackageSpec::pinned("@scope/tool", "2.1.0"),
            ]
        );
    }

    #[test]
    fn the_banner_and_binaries_are_not_packages() {
        let names: Vec<String> = yarn()
            .parse_list(LIST)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names.len(), 2);
        for noise in ["yarn", "Done", "-", "prettier@3.9.9"] {
            assert!(!names.contains(&noise.to_string()), "parsed `{noise}`");
        }
    }

    #[test]
    fn a_scoped_name_keeps_its_leading_at() {
        let scoped = yarn()
            .parse_list(LIST)
            .into_iter()
            .find(|spec| spec.name.starts_with('@'))
            .expect("a scope survives");
        assert_eq!(scoped.version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn packages_are_added_and_removed_globally() {
        let add = yarn().install_commands(&[PackageSpec::pinned("prettier", "3.9.9")]);
        assert_eq!(add[0].args, vec!["global", "add", "prettier@3.9.9"]);
        let remove = yarn().uninstall_commands(&[PackageSpec::new("prettier")]);
        assert_eq!(remove[0].args, vec!["global", "remove", "prettier"]);
    }
}
