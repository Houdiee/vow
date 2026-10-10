//! What it takes to bring a manager in line with what it declares: one [`Plan`]
//! that `status` shows and `switch` carries out, so a preview cannot drift.

use anyhow::{Result, anyhow, bail};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::thread;

use super::version::compare_versions;
use super::{PackageSpec, declared, is_pinned};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::exec::Invocation;
use crate::graph::Tree;
use crate::manager::{self, Manager};

/// The changes that would bring one manager in line with what it declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Convergence {
    pub manager: String,
    /// Declared but not installed.
    pub install: Vec<PackageSpec>,
    /// Installed at the wrong version.
    pub repin: Vec<Repin>,
    /// Installed but not declared.
    pub remove: Vec<PackageSpec>,
    /// Installed and not declared, but beyond the manager to remove, with why.
    pub unremovable: Vec<(PackageSpec, &'static str)>,
}

impl Convergence {
    pub fn has_work(&self) -> bool {
        !self.install.is_empty() || !self.repin.is_empty() || !self.remove.is_empty()
    }

    /// A warning for each package left in place because the manager cannot remove it.
    pub fn warnings(&self) -> Diagnostics {
        let mut warnings = Diagnostics::default();
        for (spec, why) in &self.unremovable {
            warnings.push(
                Diagnostic::warning(format!(
                    "`{}` keeps `{spec}`, which nothing declares",
                    self.manager
                ))
                .note(format!("{why}; declare it, or remove it by hand")),
            );
        }
        warnings
    }

    /// Packages to install, including repins.
    pub fn to_install(&self) -> Vec<PackageSpec> {
        let mut specs = self.install.clone();
        specs.extend(self.repin.iter().map(|change| change.spec.clone()));
        specs
    }
}

/// A declared pin that does not match the installed version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repin {
    pub spec: PackageSpec,
    pub installed: String,
}

impl Repin {
    /// `Less` means the declared version is older than the installed one.
    pub fn direction(&self) -> Option<Ordering> {
        compare_versions(self.spec.version.as_deref()?, &self.installed)
    }
}

/// One manager read in full: what it declares, and what it would take to get
/// there.
pub struct Plan {
    pub manager: &'static dyn Manager,
    pub declared: BTreeMap<String, PackageSpec>,
    pub changes: Convergence,
}

impl Plan {
    /// The commands that carry out [`Plan::changes`]. Installs come first, so an
    /// interrupted run leaves more installed rather than less.
    pub fn commands(&self) -> Vec<Invocation> {
        let mut commands = self.manager.install_commands(&self.changes.to_install());
        commands.extend(self.manager.uninstall_commands(&self.changes.remove));
        commands
    }

    /// Declared names held at a version, by a version after the name or by the name itself.
    pub fn pinned(&self) -> Vec<String> {
        self.names(|spec| is_pinned(self.manager, spec))
    }

    /// Bring every unpinned declared package up to date, leaving pins alone.
    pub fn upgrade_commands(&self) -> Vec<Invocation> {
        let unpinned = self.names(|spec| !is_pinned(self.manager, spec));
        self.manager.upgrade_commands(&unpinned, &self.pinned())
    }

    fn names(&self, keep: impl Fn(&PackageSpec) -> bool) -> Vec<String> {
        self.declared
            .values()
            .filter(|spec| keep(spec))
            .map(|spec| spec.name.clone())
            .collect()
    }
}

/// Read one manager: resolve its declarations, check it can hold them, and
/// compare them with what it has installed.
pub fn plan(tree: &Tree, manager: &'static dyn Manager) -> Result<Plan> {
    let declared = declared(tree, manager)?;
    let installed = manager::installed(manager)?;
    let changes = compute(
        manager.id(),
        &declared.declared,
        &installed,
        manager.supports_pinning(),
        &|name| manager.canonical(name),
        &|name| manager.keeps(name),
    )?;
    Ok(Plan {
        manager,
        declared: declared.declared,
        changes,
    })
}

/// [`plan`] every manager at once, since they share nothing. Results keep the
/// order asked for.
pub fn plan_all(
    tree: &Tree,
    managers: &[&'static dyn Manager],
) -> Vec<(&'static dyn Manager, Result<Plan>)> {
    thread::scope(|scope| {
        let workers: Vec<_> = managers
            .iter()
            .map(|&manager| (manager, scope.spawn(move || plan(tree, manager))))
            .collect();
        workers
            .into_iter()
            .map(|(manager, worker)| {
                let result = worker
                    .join()
                    .unwrap_or_else(|_| Err(anyhow!("the worker for `{}` panicked", manager.id())));
                (manager, result)
            })
            .collect()
    })
}

/// Compare declared with installed, keyed by canonical name. Pure, so what gets
/// uninstalled is tested directly.
pub fn compute(
    manager: &str,
    declared: &BTreeMap<String, PackageSpec>,
    installed: &BTreeMap<String, PackageSpec>,
    supports_pinning: bool,
    canonical: &dyn Fn(&str) -> String,
    keeps: &dyn Fn(&str) -> Option<&'static str>,
) -> Result<Convergence> {
    let mut changes = Convergence {
        manager: manager.to_string(),
        ..Convergence::default()
    };
    let mut unverifiable = Vec::new();

    for (name, wanted) in declared {
        match installed.get(&canonical(name)) {
            None => changes.install.push(wanted.clone()),
            Some(present) => {
                if !supports_pinning {
                    continue;
                }
                let Some(want) = &wanted.version else {
                    continue;
                };
                // A pinning manager always lists versions, so a missing one is
                // a parse failure, not a satisfied pin.
                let Some(have) = &present.version else {
                    unverifiable.push(name.clone());
                    continue;
                };
                if want != have {
                    changes.repin.push(Repin {
                        spec: wanted.clone(),
                        installed: have.clone(),
                    });
                }
            }
        }
    }

    if !unverifiable.is_empty() {
        bail!(
            "`{manager}` listed {} without a version, so the declared pin cannot be \
             verified -- its list output may have changed format",
            unverifiable.join(", ")
        );
    }

    // Whatever its spelling, a declared package is never removed.
    let wanted: std::collections::BTreeSet<String> =
        declared.keys().map(|name| canonical(name)).collect();
    for (name, present) in installed {
        if wanted.contains(name) {
            continue;
        }
        match keeps(&present.name) {
            Some(why) => changes.unremovable.push((present.clone(), why)),
            // The whole spec: some managers need the version to remove it.
            None => changes.remove.push(present.clone()),
        }
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn removed(changes: &Convergence) -> Vec<String> {
        changes
            .remove
            .iter()
            .map(|spec| spec.name.clone())
            .collect()
    }

    fn map(specs: &[PackageSpec]) -> BTreeMap<String, PackageSpec> {
        specs
            .iter()
            .map(|spec| (spec.name.clone(), spec.clone()))
            .collect()
    }

    fn changes_of(
        declared: &[PackageSpec],
        installed: &[PackageSpec],
        pinning: bool,
    ) -> Convergence {
        compute(
            "test",
            &map(declared),
            &map(installed),
            pinning,
            &str::to_string,
            &|_| None,
        )
        .expect("reconciles")
    }

    #[test]
    fn install_what_is_missing_and_remove_what_is_undeclared() {
        let vim = [PackageSpec::new("vim")];
        assert!(!changes_of(&vim, &vim, false).has_work());

        let changes = changes_of(&vim, &[PackageSpec::new("nano")], false);
        assert_eq!(changes.install, vim);
        assert_eq!(removed(&changes), vec!["nano"]);
    }

    #[test]
    fn a_declared_package_listed_under_another_spelling_is_kept() {
        let pip = manager::get("pip").expect("registered");
        let installed: BTreeMap<String, PackageSpec> = [
            PackageSpec::pinned("PyYAML", "6.0.1"),
            PackageSpec::pinned("typing_extensions", "4.12.2"),
        ]
        .into_iter()
        .map(|spec| (pip.canonical(&spec.name), spec))
        .collect();
        let declared = map(&[
            PackageSpec::new("pyyaml"),
            PackageSpec::new("typing-extensions"),
        ]);
        let changes = compute(
            "pip",
            &declared,
            &installed,
            true,
            &|name| pip.canonical(name),
            &|_| None,
        )
        .expect("reconciles");
        assert!(!changes.has_work(), "{changes:?}");
    }

    #[test]
    fn what_the_manager_cannot_remove_is_left_and_warned_about() {
        let volta = manager::get("volta").expect("registered");
        let changes = compute(
            "volta",
            &BTreeMap::new(),
            &map(&[
                PackageSpec::pinned("node", "22.23.3"),
                PackageSpec::new("tsc"),
            ]),
            true,
            &str::to_string,
            &|name| volta.keeps(name),
        )
        .expect("reconciles");
        assert_eq!(removed(&changes), vec!["tsc"]);
        assert_eq!(changes.unremovable.len(), 1);
        assert!(changes.warnings().to_string().contains("`node 22.23.3`"));
    }

    #[test]
    fn a_pin_mismatch_is_a_repin() {
        let changes = changes_of(
            &[PackageSpec::pinned("ripgrep", "14.1.0")],
            &[PackageSpec::pinned("ripgrep", "14.0.0")],
            true,
        );
        assert_eq!(
            changes.repin,
            vec![Repin {
                spec: PackageSpec::pinned("ripgrep", "14.1.0"),
                installed: "14.0.0".into()
            }]
        );
        assert!(changes.install.is_empty());
        assert!(changes.remove.is_empty());
    }

    #[test]
    fn an_unverifiable_pin_is_an_error_not_a_pass() {
        let error = compute(
            "test",
            &map(&[PackageSpec::pinned("ripgrep", "14.1.0")]),
            &map(&[PackageSpec::new("ripgrep")]),
            true,
            &str::to_string,
            &|_| None,
        )
        .expect_err("must refuse to guess");
        assert!(error.to_string().contains("ripgrep"));
        assert!(error.to_string().contains("cannot be verified"));
    }

    #[test]
    fn repins_are_installed_alongside_new_packages() {
        let changes = changes_of(
            &[
                PackageSpec::new("bat"),
                PackageSpec::pinned("ripgrep", "14.1.0"),
            ],
            &[PackageSpec::pinned("ripgrep", "14.0.0")],
            true,
        );
        assert_eq!(
            changes.to_install(),
            vec![
                PackageSpec::new("bat"),
                PackageSpec::pinned("ripgrep", "14.1.0")
            ]
        );
    }
}
