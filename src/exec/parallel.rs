//! Running managers a stage at a time, each in its own thread and its own box.

use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;
use std::thread;

use super::Invocation;
use super::live::Live;
use crate::atomic;
use crate::diagnostic::Diagnostic;
use crate::report::{self, nested, style};
use crate::state;

/// A switch's work under one live view: placing links, then each stage's
/// package commands. Every step has a box from the start, waiting in it until
/// its turn. Where boxes will not fit, each is printed under its name when it
/// finishes instead.
pub struct Run {
    live: Option<Live>,
    /// Managers not yet run, in order, to redraw after a pause.
    pending: Vec<String>,
}

impl Run {
    /// Ask for root if anything queued needs it, then draw a box for each
    /// task in `first` and then each manager, in the order given.
    pub fn start(first: &[&str], queued: &[(&str, Vec<Invocation>)]) -> Result<Self> {
        preauthorize(queued)?;
        let pending: Vec<String> = first
            .iter()
            .copied()
            .chain(queued.iter().map(|(id, _)| *id))
            .map(str::to_string)
            .collect();
        let names: Vec<&str> = pending.iter().map(String::as_str).collect();
        Ok(Self {
            live: Live::start(&names),
            pending,
        })
    }

    /// Stop drawing, leaving the boxes as they are, so something else can be
    /// printed below them.
    pub fn pause(&mut self) {
        self.live = None;
    }

    /// Draw again below, after a pause: `remaining` is every manager still
    /// to run, in the order to draw them, any added since included.
    pub fn resume(&mut self, remaining: &[(&str, Vec<Invocation>)]) -> Result<()> {
        self.pause();
        preauthorize(remaining)?;
        self.pending = remaining.iter().map(|(id, _)| id.to_string()).collect();
        let names: Vec<&str> = self.pending.iter().map(String::as_str).collect();
        self.live = Live::start(&names);
        Ok(())
    }

    /// Mark what will not run now that a stage failed.
    pub fn not_run(&mut self, managers: &[&str]) {
        if let Some(live) = &self.live {
            for manager in managers {
                live.not_run(manager);
            }
        }
        self.pending.retain(|id| !managers.contains(&id.as_str()));
    }

    /// Do work that runs no command, such as placing links, in its own box,
    /// reported as a manager is: `work` says each line it would print, and
    /// a failure is logged and ends in an error pointing at the log.
    pub fn task(&mut self, id: &str, work: impl FnOnce(&dyn Fn(&str)) -> Result<()>) -> Result<()> {
        let live = self.live.as_ref();
        if let Some(live) = live {
            live.begin(id);
        }
        let transcript = std::sync::Mutex::new(String::new());
        let say = |line: &str| {
            if let Ok(mut transcript) = transcript.lock() {
                transcript.push_str(&nested(1, format!("{line}\n")));
            }
            if let Some(live) = live {
                live.line(id, line);
            }
        };
        let result = work(&say);
        if let Some(live) = live {
            match &result {
                Ok(()) => live.done(id),
                Err(error) => live.failed(id, &format!("{error:#}")),
            }
        }
        self.pending.retain(|pending| pending != id);

        let mut transcript = transcript.into_inner().unwrap_or_default();
        let Err(error) = result else {
            if live.is_none() {
                println!("{} {}", report::bold(id), style::added("done"));
            }
            return Ok(());
        };
        transcript.push_str(&nested(1, format!("{error:#}\n")));
        let log = save_log(id, &transcript);
        if live.is_none() {
            println!("{} {}", report::bold(id), report::failure("failed"));
            print!("{transcript}");
        }
        let mut failed = Diagnostic::error(format!("{error:#}"));
        if let Some(log) = log {
            failed = failed.in_place(&log);
        }
        Err(failed.into())
    }

    /// Run one stage's managers at once. One failing does not stop the
    /// others: stopping a half-finished `cargo` is worse.
    pub fn stage(&mut self, queued: &[(&str, Vec<Invocation>)]) -> Result<()> {
        let live = self.live.as_ref();
        if let Some(live) = live {
            for (id, _) in queued {
                live.begin(id);
            }
        }
        let finished: Vec<(&str, String, Result<()>)> = thread::scope(|scope| {
            let workers: Vec<_> = queued
                .iter()
                .map(|(id, commands)| (*id, scope.spawn(move || run_one(id, commands, live))))
                .collect();
            workers
                .into_iter()
                .map(|(id, worker)| {
                    let (transcript, result) = worker.join().unwrap_or_else(|_| {
                        (
                            String::new(),
                            Err(anyhow!("the worker for `{id}` panicked")),
                        )
                    });
                    (id, transcript, result)
                })
                .collect()
        });
        self.pending
            .retain(|id| !queued.iter().any(|(ran, _)| ran == id));
        settle(&finished, live.is_some())
    }
}

/// Say how a stage went: with boxes, each already has, so only the error
/// remains; without, a line per manager and each failed one's transcript.
fn settle(finished: &[(&str, String, Result<()>)], watched: bool) -> Result<()> {
    let mut failures = Vec::new();
    let mut logged = Vec::new();
    for (id, transcript, result) in finished {
        if result.is_ok() {
            if !watched {
                println!("{} {}", report::bold(id), style::added("done"));
            }
            continue;
        }
        logged.extend(save_log(id, transcript));
        if !watched {
            println!("{} {}", report::bold(id), report::failure("failed"));
            print!("{transcript}");
        }
        failures.push(*id);
    }

    if failures.is_empty() {
        return Ok(());
    }
    failures.sort();
    let mut failed = Diagnostic::error(format!(
        "{} of {} manager(s) failed: {}",
        failures.len(),
        finished.len(),
        failures.join(", ")
    ));
    // One failure points at its log; several, at the directory of them.
    match logged.as_slice() {
        [one] => failed = failed.in_place(one),
        [] => {}
        _ => {
            if let Some(logs) = logs() {
                failed = failed.in_place(&logs);
            }
        }
    }
    Err(failed.into())
}

/// One manager's commands in order, stopping at the first that fails.
fn run_one(id: &str, commands: &[Invocation], live: Option<&Live>) -> (String, Result<()>) {
    let mut transcript = String::new();

    for command in commands {
        transcript.push_str(&nested(1, format!("$ {}\n", command.display())));
        let (output, outcome) = match live {
            Some(live) => {
                live.line(id, &format!("$ {}", command.display()));
                command.run_streamed(&|line| live.line(id, line))
            }
            None => command.run_captured(),
        };
        transcript.push_str(&indent(&output));

        if let Err(error) = outcome {
            if let Some(live) = live {
                live.failed(id, &format!("{error:#}"));
            }
            return (transcript, Err(error));
        }
    }

    if let Some(live) = live {
        live.done(id);
    }
    (transcript, Ok(()))
}

/// Ask for root before the boxes are drawn: a password prompt inside a worker
/// would block with its output captured and nothing on screen to explain why.
fn preauthorize(queued: &[(&str, Vec<Invocation>)]) -> Result<()> {
    let needs_root = queued
        .iter()
        .flat_map(|(_, commands)| commands)
        .any(|command| command.needs_root);
    if !needs_root {
        return Ok(());
    }
    Invocation::new("true")
        .with_root()
        .run()
        .context("could not obtain root")
}

/// A command's output, nested under the command.
fn indent(output: &str) -> String {
    output
        .lines()
        .map(|line| nested(2, format!("{line}\n")))
        .collect()
}

/// Where failed managers' output is kept.
fn logs() -> Option<PathBuf> {
    Some(state::home()?.join("logs"))
}

/// Keep a failed manager's whole output, one file per manager, overwritten
/// each run.
fn save_log(manager: &str, text: &str) -> Option<PathBuf> {
    let path = logs()?.join(format!("{manager}.log"));
    atomic::write(&path, text).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_needing_root_asks_for_nothing() {
        let queued = vec![("cargo", vec![Invocation::new("true")])];
        preauthorize(&queued).expect("no elevation, no prompt");
    }

    #[test]
    fn a_manager_stops_at_its_first_failure() {
        let commands = [
            Invocation::new("sh").args(["-c", "echo first"]),
            Invocation::new("sh").args(["-c", "echo boom >&2; exit 1"]),
            Invocation::new("sh").args(["-c", "echo never"]),
        ];
        let (transcript, result) = run_one("cargo", &commands, None);
        assert!(result.is_err());
        assert!(
            transcript.starts_with("  $ sh -c 'echo first'\n    first\n"),
            "{transcript}"
        );
        assert!(transcript.contains("boom") && !transcript.contains("never"));
    }
}
