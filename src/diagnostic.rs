use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use annotate_snippets::{AnnotationKind, Group, Level, Origin, Renderer, Snippet};

use crate::report;

/// How many faults to draw before saying how many are left.
const MOST: usize = 10;

/// Where something is in a file, as a byte range.
pub type Span = Range<usize>;

/// The range `piece` occupies in `text`, found by address since `piece` is
/// normally a slice of it.
pub fn at(text: &str, piece: &str) -> Span {
    let start = within(text, piece)
        .or_else(|| text.find(piece))
        .unwrap_or(0);
    start..start + piece.len()
}

/// The same, ignoring the whitespace around `piece`.
pub fn at_trimmed(text: &str, piece: &str) -> Span {
    at(text, piece.trim())
}

/// The line number something starts on, given all the text before it.
/// Counted by newlines: `lines()` would put the start of line 2 on line 1.
fn line_of(before: &str) -> usize {
    before.matches('\n').count() + 1
}

fn within(text: &str, piece: &str) -> Option<usize> {
    let offset = (piece.as_ptr() as usize).checked_sub(text.as_ptr() as usize)?;
    (offset + piece.len() <= text.len()).then_some(offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Error,
    Warning,
}

/// A file as it was read, so a fault found after parsing can still quote it.
#[derive(Debug, Clone)]
pub struct Source {
    path: Arc<Path>,
    text: Arc<str>,
}

impl Source {
    pub fn new(path: &Path, text: &Arc<str>) -> Self {
        Self {
            path: Arc::from(path),
            text: Arc::clone(text),
        }
    }

    pub fn fault(&self, diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.in_file(&self.path, &self.text)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// `path:line` for a span, to name a place in this file from a fault
    /// drawn in another.
    pub fn locate(&self, span: &Span) -> String {
        let line = line_of(&self.text[..span.start.min(self.text.len())]);
        format!("{}:{line}", report::path(&self.path))
    }
}

/// A fault in something a person wrote, with where they wrote it. Built in
/// pieces: a parser knows what and where, only its caller knows which file.
#[derive(Debug, Clone)]
pub struct Diagnostic(Box<Report>);

#[derive(Debug, Clone)]
struct Report {
    severity: Severity,
    message: String,
    /// Where the fault is, and what the arrow says.
    primary: Option<(Span, String)>,
    /// Places that explain it rather than being it.
    related: Vec<(Span, String)>,
    notes: Vec<String>,
    path: Option<PathBuf>,
    text: Option<Arc<str>>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self(Box::new(Report {
            severity,
            message: message.into(),
            primary: None,
            related: Vec::new(),
            notes: Vec::new(),
            path: None,
            text: None,
        }))
    }

    pub fn at(mut self, span: Span, label: impl Into<String>) -> Self {
        self.0.primary = Some((span, label.into()));
        self
    }

    pub fn and(mut self, span: Span, label: impl Into<String>) -> Self {
        self.0.related.push((span, label.into()));
        self
    }

    /// Why, where the rule is not obvious from the message.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.0.notes.push(note.into());
        self
    }

    /// Attach the file the spans refer to.
    pub fn in_file(mut self, path: &Path, text: &Arc<str>) -> Self {
        self.0.path = Some(path.to_path_buf());
        self.0.text = Some(Arc::clone(text));
        self
    }

    /// Name a file without quoting it, for a fault about the file itself.
    pub fn in_place(mut self, path: &Path) -> Self {
        self.0.path = Some(path.to_path_buf());
        self
    }

    pub fn render(&self) -> String {
        self.render_with_notes(&self.0.notes)
    }

    fn render_with_notes(&self, notes: &[String]) -> String {
        let text = self.0.text.as_deref().unwrap_or_default();
        let path = self.0.path.as_deref().map(report::path);
        let level = match self.0.severity {
            Severity::Error => Level::ERROR,
            Severity::Warning => Level::WARNING,
        };
        let title = level.primary_title(self.0.message.as_str());

        let renderer = if report::colored() {
            Renderer::styled()
        } else {
            Renderer::plain()
        };

        // Nothing to frame: notes go straight under the title, rather than
        // under an empty gutter.
        if self.0.primary.is_none() && path.is_none() {
            let mut out = renderer.render(&[Group::with_title(title)]);
            out.push('\n');
            for note in notes {
                out.push_str(&format!("  {} {note}\n", report::frame("= note:")));
            }
            return out;
        }

        let mut group = match (&self.0.primary, path) {
            (Some((span, label)), path) => {
                let mut snippet = Snippet::source(text).path(path).annotation(
                    AnnotationKind::Primary
                        .span(span.clone())
                        .label(label.as_str()),
                );
                for (span, label) in &self.0.related {
                    snippet = snippet.annotation(
                        AnnotationKind::Context
                            .span(span.clone())
                            .label(label.as_str()),
                    );
                }
                title.element(snippet)
            }
            (None, Some(path)) => title.element(Origin::path(path)),
            (None, None) => Group::with_title(title),
        };

        for note in notes {
            group = group.element(Level::NOTE.message(note.as_str()));
        }

        let mut out = renderer.render(&[group]);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    /// `path:line:col`, for the one-line form.
    fn location(&self) -> Option<String> {
        let path = report::path(self.0.path.as_ref()?);
        let (Some((span, _)), Some(text)) = (&self.0.primary, &self.0.text) else {
            return Some(path);
        };
        let before = text.get(..span.start)?;
        let line = line_of(before);
        let column = before
            .rfind('\n')
            .map_or(before, |newline| &before[newline + 1..])
            .chars()
            .count()
            + 1;
        Some(format!("{path}:{line}:{column}"))
    }
}

/// `path:line:col: message`, for anywhere that is not a terminal.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.location() {
            Some(location) => write!(f, "{location}: {}", self.0.message),
            None => f.write_str(&self.0.message),
        }
    }
}

impl std::error::Error for Diagnostic {}

/// Every fault found in one pass, so a file is reported whole rather than one
/// line per attempt.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.0.extend(other.0);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `Ok` when nothing was found, else every fault as one error.
    pub fn into_result(self) -> anyhow::Result<()> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(self.into())
        }
    }

    /// Capped at [`MOST`], with each note said once, so the first fault is not
    /// buried.
    pub fn render(&self) -> String {
        let mut said: BTreeSet<&str> = BTreeSet::new();
        let mut out: String = self
            .0
            .iter()
            .take(MOST)
            .map(|diagnostic| {
                let fresh: Vec<String> = diagnostic
                    .0
                    .notes
                    .iter()
                    .filter(|note| said.insert(note.as_str()))
                    .cloned()
                    .collect();
                diagnostic.render_with_notes(&fresh)
            })
            .collect::<Vec<String>>()
            .join("\n");
        if let Some(rest) = self.0.len().checked_sub(MOST).filter(|rest| *rest > 0) {
            out.push_str(&format!(
                "\n{}\n",
                report::dim(&format!("... {rest} more not shown"))
            ));
        }
        out
    }

    /// The arrowed form of an error, when it carries diagnostics at all.
    pub fn of(error: &anyhow::Error) -> Option<String> {
        if let Some(set) = error.downcast_ref::<Diagnostics>() {
            return Some(set.render());
        }
        error.downcast_ref::<Diagnostic>().map(Diagnostic::render)
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Self {
        Self(vec![diagnostic])
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: Vec<String> = self.0.iter().map(Diagnostic::to_string).collect();
        f.write_str(&text.join("\n"))
    }
}

impl std::error::Error for Diagnostics {}
