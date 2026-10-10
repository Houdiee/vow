//! Where a declared package was written, so a fault found later -- a conflict
//! with another file, a version the manager cannot hold -- can point at it.

use toml::Spanned;

use super::PackageSpec;
use crate::diagnostic::{Diagnostic, Source, Span, at};
use crate::graph::File;

#[derive(Debug, Clone)]
pub struct Origin {
    /// `None` for a declaration built in memory.
    source: Option<Source>,
    at: Span,
    version: Option<Span>,
}

impl Origin {
    /// `path:line`, to name this declaration from a fault drawn elsewhere.
    pub fn locate(&self) -> String {
        match &self.source {
            Some(source) => source.locate(&self.at),
            None => "an unsaved declaration".to_string(),
        }
    }

    /// Point a fault at the whole declaration.
    pub fn fault(&self, diagnostic: Diagnostic, label: impl Into<String>) -> Diagnostic {
        self.place(diagnostic, self.at.clone(), label)
    }

    /// Point a fault at the version.
    pub fn fault_version(&self, diagnostic: Diagnostic, label: impl Into<String>) -> Diagnostic {
        let span = self.version.clone().unwrap_or(self.at.clone());
        self.place(diagnostic, span, label)
    }

    /// Show this declaration beside a fault about another in the same file.
    pub fn also(&self, diagnostic: Diagnostic, label: impl Into<String>) -> Diagnostic {
        match &self.source {
            Some(_) => diagnostic.and(self.at.clone(), label),
            None => diagnostic,
        }
    }

    fn place(&self, diagnostic: Diagnostic, span: Span, label: impl Into<String>) -> Diagnostic {
        match &self.source {
            Some(source) => source.fault(diagnostic.at(span, label)),
            // No file to draw on, so the label becomes a note.
            None => diagnostic.note(label),
        }
    }

    #[cfg(test)]
    pub fn detached() -> Self {
        Self {
            source: None,
            at: 0..0,
            version: None,
        }
    }
}

/// One package as `file` declares it, in a string such as `"bat 0.24.0"`.
pub fn read(file: &File, item: &Spanned<String>) -> Result<(PackageSpec, Origin), Diagnostic> {
    let text = file.source.text();
    let span = item.span();
    let value = item.get_ref().as_str();
    // The string's contents as a slice of the file, so each fault points at
    // its own field; one written with escapes is located as a whole.
    let inner = text
        .get(span.start + 1..span.end.saturating_sub(1))
        .filter(|inner| *inner == value);
    let (spec, version) = match inner {
        Some(inner) => {
            let spec = PackageSpec::read(text, inner).map_err(|fault| file.source.fault(fault))?;
            let version = spec
                .version
                .as_deref()
                .and_then(|version| inner.split_whitespace().find(|field| *field == version))
                .map(|field| at(text, field));
            (spec, version)
        }
        None => {
            let spec = PackageSpec::read(value, value)
                .map_err(|fault| file.source.fault(fault.at(span.clone(), "here")))?;
            (spec, None)
        }
    };
    let origin = Origin {
        source: Some(file.source.clone()),
        at: span,
        version,
    };
    Ok((spec, origin))
}
