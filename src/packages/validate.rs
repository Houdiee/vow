//! What a manager can be asked to hold: a version only where it can be held.

use anyhow::Result;
use std::collections::BTreeMap;

use super::{PackageSpec, Resolved, resolve};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::graph::Tree;
use crate::manager::Manager;

/// Reject pins this manager cannot honour, rather than install some other
/// version.
fn validate(manager: &dyn Manager, resolved: &Resolved) -> Result<()> {
    let mut problems = Diagnostics::default();
    let fault = |name: &str, problem: Diagnostic, label: &str| match resolved.origins.get(name) {
        Some(origin) => origin.fault(problem, label.to_string()),
        None => problem,
    };

    // Two spellings of one package would both read back as installed, and
    // could disagree on its version.
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for name in resolved.declared.keys() {
        if let Some(fix) = manager.refuses_name(name) {
            problems.push(
                fault(
                    name,
                    Diagnostic::error(format!(
                        "{} cannot hold `{name}` by that name",
                        manager.id()
                    )),
                    "not a package name",
                )
                .note(fix),
            );
        }
        if let Some(other) = seen.insert(manager.canonical(name), name) {
            problems.push(
                fault(
                    name,
                    Diagnostic::error(format!(
                        "`{other}` and `{name}` are the same package to {}",
                        manager.id()
                    )),
                    "declared again",
                )
                .note("keep one spelling"),
            );
        }
    }

    for (name, spec) in &resolved.declared {
        if spec.version.is_none() {
            continue;
        }

        let problem = if manager.name_selects_version(name) {
            Diagnostic::error(format!("the name `{name}` already selects a version"))
                .note("a name that carries its own version is pinned by itself")
        } else if !manager.supports_pinning() {
            Diagnostic::error(format!(
                "{} cannot install a specific version of `{name}`",
                manager.id()
            ))
            .note("only a manager that can reinstall an exact version later accepts one")
        } else {
            continue;
        };

        problems.push(match resolved.origins.get(name) {
            Some(origin) => origin.fault_version(problem, "drop the version"),
            None => problem.note("drop the version"),
        });
    }

    problems.into_result()
}

/// What the files declare for `manager`, merged and checked against what it
/// can hold: valid or not whether or not it is installed here.
pub fn declared(tree: &Tree, manager: &dyn Manager) -> Result<Resolved> {
    let resolved = resolve(tree, manager.id())?;
    validate(manager, &resolved)?;
    Ok(resolved)
}

/// Whether a declaration fixes a version: with a version after the name, or with a
/// name that selects one, such as brew's `node@20` or pyenv's `3.12.8`.
pub fn is_pinned(manager: &dyn Manager, spec: &PackageSpec) -> bool {
    spec.version.is_some() || manager.name_selects_version(&spec.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::{self, Manager};
    use crate::packages::Origin;

    fn get(id: &str) -> &'static dyn Manager {
        manager::get(id).expect("registered")
    }

    fn declared(specs: &[PackageSpec]) -> Resolved {
        Resolved {
            declared: specs
                .iter()
                .map(|spec| (spec.name.clone(), spec.clone()))
                .collect(),
            origins: specs
                .iter()
                .map(|spec| (spec.name.clone(), Origin::detached()))
                .collect(),
        }
    }

    #[test]
    fn a_name_that_selects_a_version_counts_as_pinned() {
        assert!(is_pinned(get("pyenv"), &PackageSpec::new("3.12.8")));
        assert!(is_pinned(get("brew"), &PackageSpec::new("node@20")));
        assert!(!is_pinned(get("brew"), &PackageSpec::new("ripgrep")));
    }

    #[test]
    fn a_pin_on_a_manager_that_cannot_pin_is_an_error() {
        let error = validate(
            get("brew"),
            &declared(&[PackageSpec::pinned("ripgrep", "14.1.0")]),
        )
        .expect_err("must reject");
        assert!(
            error
                .to_string()
                .contains("cannot install a specific version")
        );
    }

    #[test]
    fn a_pin_on_a_manager_that_can_pin_is_accepted() {
        validate(
            get("cargo"),
            &declared(&[PackageSpec::pinned("ripgrep", "14.1.0")]),
        )
        .expect("accepted");
    }

    #[test]
    fn a_versioned_formula_plus_a_pin_is_a_contradiction() {
        let error = validate(
            get("brew"),
            &declared(&[PackageSpec::pinned("node@20", "20.11.0")]),
        )
        .expect_err("must reject");
        assert!(error.to_string().contains("already selects a version"));
    }

    #[test]
    fn unpinned_entries_are_always_fine() {
        for manager in manager::all() {
            validate(manager, &declared(&[PackageSpec::new("vim")])).unwrap_or_else(|error| {
                panic!("`{}` rejected an unpinned package: {error}", manager.id())
            });
        }
    }

    #[test]
    fn a_name_the_manager_reads_as_another_is_refused() {
        let error = validate(get("npm"), &declared(&[PackageSpec::new("okpkg@latest")]))
            .expect_err("a tag is not a name");
        assert!(error.to_string().contains("okpkg@latest"));
        validate(get("npm"), &declared(&[PackageSpec::new("@scope/pkg")])).expect("a scope is");

        let error = validate(
            get("pip"),
            &declared(&[PackageSpec::new("PyYAML"), PackageSpec::new("pyyaml")]),
        )
        .expect_err("one package");
        assert!(error.to_string().contains("the same package"));
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let error = validate(
            get("brew"),
            &declared(&[
                PackageSpec::pinned("ripgrep", "14.1.0"),
                PackageSpec::pinned("bat", "0.24.0"),
            ]),
        )
        .expect_err("must reject");
        assert!(error.to_string().contains("ripgrep"));
        assert!(error.to_string().contains("bat"));
    }
}
