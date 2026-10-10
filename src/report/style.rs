//! Colour, and whether the terminal can draw boxes.

use std::io::IsTerminal;
use std::sync::OnceLock;

pub fn colored() -> bool {
    static COLORED: OnceLock<bool> = OnceLock::new();
    *COLORED.get_or_init(|| {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        std::io::stdout().is_terminal()
    })
}

/// Whether box-drawing characters will render: a UTF-8 locale, and not the
/// Linux console, whose fonts carry only part of the set.
pub fn unicode() -> bool {
    static UNICODE: OnceLock<bool> = OnceLock::new();
    *UNICODE.get_or_init(|| {
        if std::env::var("TERM").is_ok_and(|term| term == "linux" || term == "dumb") {
            return false;
        }
        ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("utf-8") || value.contains("utf8")
            })
    })
}

fn paint(code: &str, text: &str) -> String {
    if colored() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// Green adds, red takes away or failed, yellow changes something or warns,
// cyan marks what applies here; bold and dim say what to read first.

pub fn bold(text: &str) -> String {
    paint("1", text)
}

pub fn dim(text: &str) -> String {
    paint("2", text)
}

/// Wrong, but not fatal.
pub fn problem(text: &str) -> String {
    paint("33", text)
}

pub fn accent(text: &str) -> String {
    paint("36", text)
}

pub fn added(text: &str) -> String {
    paint("32", text)
}

pub fn failure(text: &str) -> String {
    paint("1;31", text)
}

pub fn removed(text: &str) -> String {
    paint("31", text)
}

pub fn changed(text: &str) -> String {
    paint("33", text)
}

/// A diagnostic frame marker, as one escape: a nested reset would end the bold.
pub fn frame(text: &str) -> String {
    paint("1;34", text)
}
