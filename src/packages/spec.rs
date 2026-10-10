//! The grammar of one declared package: a name, and maybe a version.

use anyhow::Result;
use std::fmt;

use crate::diagnostic::{Diagnostic, at, at_trimmed};

/// A package as declared: a name, optionally pinned to a version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageSpec {
    pub name: String,
    pub version: Option<String>,
}

impl PackageSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }

    pub fn pinned(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: Some(version.into()),
        }
    }

    /// Parse `<name>` or `<name> <version>` from a command-line argument.
    /// Whitespace is the only separator -- no manager allows it in a name --
    /// so `node@20` is a name, not a pinned `node`.
    #[cfg(test)]
    pub fn parse(text: &str) -> Result<Self> {
        Self::read(text, text).map_err(anyhow::Error::from)
    }

    /// The same, from a slice of `file`, with each fault located in it.
    pub fn read(file: &str, declaration: &str) -> Result<Self, Diagnostic> {
        let mut fields = declaration.split_whitespace();
        let Some(name) = fields.next() else {
            return Err(Diagnostic::error("an empty package")
                .at(at_trimmed(file, declaration), "expected a package name"));
        };
        let span = |field: &str| at(file, field);

        // It would reach the manager as an option.
        if name.starts_with('-') {
            return Err(Diagnostic::error("a package name cannot start with `-`")
                .at(span(name), "this would reach the manager as an option")
                .note(
                    "`pacman -S --noconfirm` is a very different command from installing \
                     a package called `--noconfirm`",
                ));
        }

        let version = fields.next();
        if let Some(extra) = fields.next() {
            return Err(Diagnostic::error("expected `<name>` or `<name> <version>`")
                .at(span(extra), "a third field")
                .note("whitespace is the only separator, so a name cannot contain a space"));
        }

        Ok(match version {
            Some(version) => Self::pinned(name, version),
            None => Self::new(name),
        })
    }
}

impl fmt::Display for PackageSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(version) => write!(f, "{} {}", self.name, version),
            None => f.write_str(&self.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> PackageSpec {
        PackageSpec::parse(text).expect("parses")
    }

    #[test]
    fn a_name_is_opaque_and_round_trips() {
        assert_eq!(spec("node@20"), PackageSpec::new("node@20"));
        assert_eq!(spec("@scope/pkg"), PackageSpec::new("@scope/pkg"));
        for text in ["ripgrep", "ripgrep 14.1.0", "node@20", "@scope/pkg 1.0.0"] {
            assert_eq!(spec(text).to_string(), text);
        }
    }
}
