//! The live view of a switch: one box per manager, redrawn in place.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::report::style;

/// How many lines of a manager's output to keep on screen.
const DEEP: usize = 5;

/// Columns a tab in a manager's output expands to.
const TAB_WIDTH: usize = 4;

/// How often the boxes are redrawn.
const FRAME: Duration = Duration::from_millis(80);

/// A box per manager, redrawn in place so managers running at once never write
/// over each other. Every pane has a fixed height, so a frame is drawn by
/// moving the cursor back to the top and rewriting every row.
pub struct Live {
    panes: Arc<Mutex<Vec<Pane>>>,
    running: Arc<AtomicBool>,
    painter: Option<JoinHandle<()>>,
    deep: usize,
}

struct Pane {
    manager: String,
    recent: VecDeque<String>,
    state: State,
}

#[derive(PartialEq, Eq)]
enum State {
    /// Its stage has not started: an earlier one is still running.
    Waiting,
    Running,
    Done,
    Failed,
    /// An earlier stage failed, so it never started.
    NotRun,
}

impl Live {
    /// A box for each of `managers`, each waiting until [`Self::begin`].
    /// `None` when the terminal cannot hold even one-line boxes; the depth
    /// shrinks from [`DEEP`] to fit before giving up.
    pub fn start(managers: &[&str]) -> Option<Self> {
        if managers.is_empty() || !std::io::stdout().is_terminal() || !style::colored() {
            return None;
        }
        let (_, rows) = size()?;
        // A body plus header and footer per pane, and a row for the cursor.
        let deep = (1..=DEEP)
            .rev()
            .find(|deep| managers.len() * (deep + 2) < rows)?;

        let panes = Arc::new(Mutex::new(
            managers
                .iter()
                .map(|manager| Pane {
                    manager: manager.to_string(),
                    recent: VecDeque::new(),
                    state: State::Waiting,
                })
                .collect(),
        ));
        let running = Arc::new(AtomicBool::new(true));

        let painter = {
            let panes = Arc::clone(&panes);
            let running = Arc::clone(&running);
            thread::spawn(move || {
                let mut drawn = 0;
                let mut was = 0;
                loop {
                    let width = size().map_or(0, |(width, _)| width);
                    // A resize rewraps the screen: start a fresh region
                    // rather than overwrite rows that are no longer ours.
                    if width != was {
                        drawn = 0;
                        was = width;
                    }
                    drawn = paint(&panes, deep, width, drawn);

                    // Stopping after a frame leaves the final state on screen.
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(FRAME);
                }
            })
        };

        Some(Self {
            panes,
            running,
            painter: Some(painter),
            deep,
        })
    }

    /// Record a line of a manager's output. Oldest lines fall off the top.
    pub fn line(&self, manager: &str, text: &str) {
        let text = readable(text);
        if text.is_empty() {
            return;
        }
        self.update(manager, |pane| pane.recent.push_back(text));
    }

    /// Its stage has started.
    pub fn begin(&self, manager: &str) {
        self.settle(manager, State::Running, None);
    }

    pub fn not_run(&self, manager: &str) {
        self.settle(manager, State::NotRun, None);
    }

    pub fn done(&self, manager: &str) {
        self.settle(manager, State::Done, None);
    }

    /// The reason goes in the box, so an early failure shows why while the
    /// others carry on.
    pub fn failed(&self, manager: &str, reason: &str) {
        self.settle(manager, State::Failed, Some(reason));
    }

    fn settle(&self, manager: &str, state: State, reason: Option<&str>) {
        self.update(manager, |pane| {
            if let Some(reason) = reason {
                pane.recent.push_back(readable(reason));
            }
            pane.state = state;
        });
    }

    /// Change one manager's pane, keeping only its last `deep` lines.
    fn update(&self, manager: &str, change: impl FnOnce(&mut Pane)) {
        if let Ok(mut panes) = self.panes.lock()
            && let Some(pane) = panes.iter_mut().find(|pane| pane.manager == manager)
        {
            change(pane);
            while pane.recent.len() > self.deep {
                pane.recent.pop_front();
            }
        }
    }
}

/// Stops the painter after a last frame, leaving the boxes on screen.
impl Drop for Live {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(painter) = self.painter.take() {
            let _ = painter.join();
        }
    }
}

/// Draw one frame over the last one and return how many rows it used. Rows
/// are cleared first, since this frame may be shorter.
fn paint(panes: &Mutex<Vec<Pane>>, deep: usize, width: usize, drawn: usize) -> usize {
    if width == 0 {
        return drawn;
    }
    let Ok(panes) = panes.lock() else {
        return drawn;
    };

    let mut out = String::new();
    if drawn > 0 {
        out.push_str(&format!("\x1b[{drawn}A"));
    }

    let mut rows = 0;
    for pane in panes.iter() {
        for line in pane.draw(width, deep) {
            out.push_str("\x1b[2K");
            out.push_str(&line);
            out.push('\n');
            rows += 1;
        }
    }

    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    rows
}

impl Pane {
    /// Exactly `deep + 2` rows -- a titled rule, the body, a rule -- since a
    /// pane that changed height would shift every pane below it.
    fn draw(&self, width: usize, deep: usize) -> Vec<String> {
        let inner = width.saturating_sub(4).max(8);
        let mut rows = Vec::with_capacity(deep + 2);

        let name = clip(&self.manager, inner.saturating_sub(8));
        let word = match self.state {
            State::Waiting => Some(("waiting", style::dim as fn(&str) -> String)),
            State::Running => None,
            State::Done => Some(("done", style::added as fn(&str) -> String)),
            State::Failed => Some(("failed", style::failure as fn(&str) -> String)),
            State::NotRun => Some(("not run", style::dim as fn(&str) -> String)),
        };
        let (title, painted) = match word {
            None => (name.clone(), style::bold(&name)),
            Some((word, paint)) => (
                format!("{name} {word}"),
                format!("{} {}", style::bold(&name), paint(word)),
            ),
        };
        // Every row is `inner + 4` columns; a longer title shortens the rule.
        let edge = Edge::pick();
        let rule = edge
            .flat
            .repeat((inner + 4).saturating_sub(count(&title) + 5).max(1));
        rows.push(
            style::dim(&format!("{}{} ", edge.open, edge.flat))
                + &painted
                + &style::dim(&format!(" {rule}{}", edge.shut)),
        );

        for at in 0..deep {
            let text = self
                .recent
                .get(at)
                .map(|line| clip(line, inner))
                .unwrap_or_default();
            let pad = " ".repeat(inner.saturating_sub(count(&text)));
            rows.push(format!(
                "{} {}{} {}",
                style::dim(edge.side),
                style::dim(&text),
                pad,
                style::dim(edge.side)
            ));
        }

        rows.push(style::dim(&format!(
            "{}{}{}",
            edge.foot,
            edge.flat.repeat(inner + 2),
            edge.tail
        )));
        rows
    }
}

/// Box-drawing characters where the terminal renders them, else `+-|`.
struct Edge {
    open: &'static str,
    shut: &'static str,
    foot: &'static str,
    tail: &'static str,
    side: &'static str,
    flat: &'static str,
}

impl Edge {
    fn pick() -> Self {
        if style::unicode() {
            Self {
                open: "╭",
                shut: "╮",
                foot: "╰",
                tail: "╯",
                side: "│",
                flat: "─",
            }
        } else {
            Self {
                open: "+",
                shut: "+",
                foot: "+",
                tail: "+",
                side: "|",
                flat: "-",
            }
        }
    }
}

/// A manager's line without escapes or control characters, which could move
/// the cursor out of its pane.
fn readable(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if ch == '\t' {
            out.push_str(&" ".repeat(TAB_WIDTH));
        } else if !ch.is_control() {
            out.push(ch);
        }
    }
    out.trim_end().to_string()
}

/// Cut to `room` columns, ending in `~` where something was cut.
fn clip(text: &str, room: usize) -> String {
    if count(text) <= room {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let wide = ch.width().unwrap_or(0);
        if used + wide + 1 > room {
            break;
        }
        out.push(ch);
        used += wide;
    }
    out.push('~');
    out
}

/// Columns on screen, not characters: `安` takes two. A row padded by
/// characters would wrap and shift every pane below it.
fn count(text: &str) -> usize {
    text.width()
}

#[cfg(unix)]
fn size() -> Option<(usize, usize)> {
    let mut window: libc::winsize = unsafe { std::mem::zeroed() };
    let answered = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut window) };
    let (cols, rows) = (window.ws_col as usize, window.ws_row as usize);
    (answered == 0 && cols > 0 && rows > 0).then_some((cols, rows))
}

#[cfg(not(unix))]
fn size() -> Option<(usize, usize)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(lines: &[&str], state: State) -> Pane {
        Pane {
            manager: "code-server".to_string(),
            recent: lines.iter().map(|line| readable(line)).collect(),
            state,
        }
    }

    #[test]
    fn a_pane_has_a_fixed_height() {
        assert_eq!(pane(&[], State::Running).draw(60, 5).len(), 7);
        assert_eq!(pane(&["installing"], State::Running).draw(60, 5).len(), 7);
    }

    #[test]
    fn a_box_says_whether_its_manager_waits_runs_or_ended() {
        let title = |state| readable(&pane(&[], state).draw(60, 1)[0]);
        assert!(
            title(State::Waiting).contains(" code-server waiting "),
            "{}",
            title(State::Waiting)
        );
        assert!(title(State::Running).contains(" code-server "));
        assert!(!title(State::Running).contains("waiting"));
        assert!(title(State::Done).contains(" code-server done "));
        assert!(title(State::NotRun).contains(" code-server not run "));
    }

    #[test]
    fn every_row_fills_the_width_whatever_a_manager_prints() {
        let lines = [
            "short",
            "安装中 100% 安装中 100% 安装中 100% 安装中 100%",
            "resolving 📦📦📦 dependencies",
            "e\u{301}\u{302} combining marks",
            &"x".repeat(400),
        ];
        for state in [State::Running, State::Done, State::Failed] {
            for row in pane(&lines, state).draw(40, 5) {
                // Measured by the width crate directly, not through `count`.
                assert_eq!(UnicodeWidthStr::width(readable(&row).as_str()), 40, "{row}");
            }
        }
    }

    #[test]
    fn escapes_and_control_characters_are_dropped() {
        assert_eq!(readable("\x1b[32mdone\x1b[0m"), "done");
        assert_eq!(readable("\x1b[2K\x1b[1Gdownloading"), "downloading");
        assert_eq!(readable("a\u{7}b\tc"), "ab    c");
    }

    #[test]
    fn a_long_line_is_cut_to_the_width_in_columns() {
        assert_eq!(clip("installing", 6), "insta~");
        assert_eq!(clip("short", 20), "short");
        assert!(count(&clip("安装中 100%", 7)) <= 7);
    }
}
