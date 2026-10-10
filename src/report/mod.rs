//! Everything vow prints: colour, plans, faults and consent.

mod links;
mod plan;
pub mod style;

pub use links::{every_link, links, unlinked};
pub use plan::{plan, summarize};
pub use style::{accent, bold, colored, dim, failure, frame};

use anyhow::Result;
use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::path::Path;

use crate::diagnostic::{Diagnostic, Diagnostics};

/// One level of nesting under a heading.
const INDENT: &str = "  ";

/// `text` nested `depth` levels under the heading above it.
pub fn nested(depth: usize, text: impl Display) -> String {
    format!("{}{text}", INDENT.repeat(depth))
}

/// A row led by a one-column marker such as `+`, `-` or `~`. Without one the
/// column is left blank, so marked and unmarked rows line up.
pub fn marked(mark: Option<String>, text: impl Display) -> String {
    format!("{} {text}", mark.unwrap_or_else(|| " ".to_string()))
}

/// Print `text` nested `depth` levels deep.
pub fn item(depth: usize, text: impl Display) {
    println!("{}", nested(depth, text));
}

/// A `note:` line: worth saying, not worth stopping for.
pub fn note(text: impl Display) {
    println!("{} {text}", accent("note:"));
}

/// Flush stdout first so a report and its faults stay in order through a pipe.
fn to_stderr(text: &str) {
    let _ = std::io::stdout().flush();
    eprint!("{text}");
}

/// A fault with a place gets its line and an arrow; anything else gets one line.
pub fn fail(error: &anyhow::Error) {
    let Some(frames) = Diagnostics::of(error) else {
        to_stderr(&format!("{} {error:#}\n", bold("error:")));
        return;
    };

    // Context around a located fault still says what vow was doing.
    let mut out = frames;
    for cause in error.chain() {
        if cause.downcast_ref::<Diagnostics>().is_some()
            || cause.downcast_ref::<Diagnostic>().is_some()
        {
            break;
        }
        out.push_str(&format!("{} {cause}\n", frame("= note:")));
    }
    to_stderr(&out);
}

/// The same, under a heading -- unless the fault names its own file.
pub fn fail_under(heading: &str, error: &anyhow::Error) {
    match Diagnostics::of(error) {
        Some(frames) => to_stderr(&frames),
        None => to_stderr(&format!("{} {error:#}\n", bold(&format!("{heading}:")))),
    }
}

/// Faults in one write, so they never interleave with the report below.
pub fn emit(diagnostics: &Diagnostics) {
    if diagnostics.is_empty() {
        return;
    }
    to_stderr(&format!("{}\n", diagnostics.render()));
}

/// A path with the home directory written as `~`.
pub fn path(path: &Path) -> String {
    let text = path.display().to_string();
    let Some(home) = dirs::home_dir() else {
        return text;
    };
    let home = home.display().to_string();
    if home.is_empty() || home == "/" {
        return text;
    }
    match text.strip_prefix(&home) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => text,
    }
}

/// Ask yes or no. A stdin that is not a terminal answers no: consent is never
/// inferred, so a pipeline passes `--yes`.
pub fn confirm(question: &str) -> Result<bool> {
    // Nobody to answer is not a no: a script would read that as success.
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("nothing changed: there is no terminal to ask; pass `--yes` to go ahead");
    }

    print!("{question} [y/N] ");
    std::io::stdout().flush()?;

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        println!();
        return Ok(false);
    }
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}
