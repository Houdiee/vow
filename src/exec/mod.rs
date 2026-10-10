//! Running commands: built as argument vectors, never as shell strings.

mod live;
mod parallel;

pub use parallel::Run;

use anyhow::{Context, Result, anyhow, bail};
use std::io::Read;
use std::process::{ChildStderr, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;

/// One of a child's two output streams.
enum Stream {
    Out(ChildStdout),
    Err(ChildStderr),
}

/// What a running command has said so far.
#[derive(Default)]
struct Heard {
    transcript: String,
    /// The last line on stderr, where tools put the complaint.
    complaint: String,
}

impl Stream {
    /// Read to the end, emitting a line at every `\n` or `\r` -- a progress
    /// bar redraws with a bare `\r`, which `BufRead::lines` would not split on.
    fn drain(self, watch: &(dyn Fn(&str) + Send + Sync), heard: &Mutex<Heard>) {
        let complains = matches!(self, Stream::Err(_));
        let mut reader: Box<dyn Read> = match self {
            Stream::Out(out) => Box::new(out),
            Stream::Err(err) => Box::new(err),
        };
        let mut pending = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            match reader.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if byte[0] != b'\n' && byte[0] != b'\r' {
                pending.push(byte[0]);
                continue;
            }
            let line = String::from_utf8_lossy(&pending).into_owned();
            pending.clear();
            keep(heard, &line, complains);
            watch(&line);
        }

        if !pending.is_empty() {
            let line = String::from_utf8_lossy(&pending).into_owned();
            keep(heard, &line, complains);
            watch(&line);
        }
    }
}

fn keep(heard: &Mutex<Heard>, line: &str, complains: bool) {
    let Ok(mut heard) = heard.lock() else { return };
    heard.transcript.push_str(line);
    heard.transcript.push('\n');
    if complains && !line.trim().is_empty() {
        heard.complaint = line.trim().to_string();
    }
}

/// A command line. Arguments go to the OS as a list, never through a shell, so
/// a package name cannot change the shape of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    /// Escalate with `sudo` (or `$VOW_SUDO`) before running.
    pub needs_root: bool,
}

impl Invocation {
    pub fn new(program: &str) -> Self {
        Self {
            program: program.to_string(),
            args: Vec::new(),
            needs_root: false,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn with_root(mut self) -> Self {
        self.needs_root = true;
        self
    }

    /// What is actually executed, after escalation.
    pub fn resolve(&self) -> (String, Vec<String>) {
        if self.needs_root
            && let Some(elevator) = elevator()
        {
            let mut args = Vec::with_capacity(self.args.len() + 1);
            args.push(self.program.clone());
            args.extend(self.args.iter().cloned());
            return (elevator, args);
        }
        (self.program.clone(), self.args.clone())
    }

    /// Run interactively, inheriting stdio so the package manager can prompt.
    pub fn run(&self) -> Result<()> {
        let (program, args) = self.resolve();
        let status = Command::new(&program)
            .args(&args)
            .status()
            .with_context(|| format!("could not launch `{program}`"))?;

        if !status.success() {
            bail!("`{}` failed: {}", self.display(), exit(status));
        }
        Ok(())
    }

    /// Run non-interactively, handing each line to `watch` as it arrives, and
    /// return both streams interleaved as they happened.
    pub fn run_streamed(&self, watch: &(dyn Fn(&str) + Send + Sync)) -> (String, Result<()>) {
        let (program, args) = self.resolve();
        let spawned = Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let reason = Err(error).with_context(|| format!("could not launch `{program}`"));
                return (String::new(), reason);
            }
        };

        let heard = Mutex::new(Heard::default());
        let out = child.stdout.take();
        let err = child.stderr.take();
        thread::scope(|scope| {
            for stream in [out.map(Stream::Out), err.map(Stream::Err)] {
                let Some(stream) = stream else { continue };
                let heard = &heard;
                scope.spawn(move || stream.drain(watch, heard));
            }
        });

        let status = child.wait();
        let heard = heard.into_inner().unwrap_or_default();
        // The complaint says what went wrong; the exit status only that it did.
        let why = |fallback: String| {
            if heard.complaint.is_empty() {
                fallback
            } else {
                heard.complaint.clone()
            }
        };
        let result = match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(anyhow!(
                "`{}` failed: {}",
                self.display(),
                why(exit(status))
            )),
            Err(error) => Err(error).with_context(|| format!("could not wait for `{program}`")),
        };
        (heard.transcript, result)
    }

    /// The same, without watching.
    pub fn run_captured(&self) -> (String, Result<()>) {
        self.run_streamed(&|_| {})
    }

    /// Run non-interactively and return stdout, failing with the complaint if
    /// the command fails.
    pub fn capture(&self) -> Result<String> {
        let output = self.output()?;
        if !output.status.success() {
            let why = last_line(&output.stderr).unwrap_or_else(|| exit(output.status));
            bail!("`{}` failed: {why}", self.display());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Stdout whatever the exit status, for a query whose status means nothing.
    pub fn capture_any_status(&self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.output()?.stdout).into_owned())
    }

    fn output(&self) -> Result<Output> {
        let (program, args) = self.resolve();
        Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("could not launch `{program}`"))
    }

    /// The command as it would be typed into a shell.
    pub fn display(&self) -> String {
        let (program, args) = self.resolve();
        let mut out = quote(&program);
        for arg in &args {
            out.push(' ');
            out.push_str(&quote(arg));
        }
        out
    }
}

/// The last line on stderr: tools put warnings first and the complaint last.
fn last_line(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// How a command ended, when it said nothing more useful.
fn exit(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => "killed by a signal".to_string(),
    }
}

/// `sudo`, or `$VOW_SUDO` (`doas`, `run0`); empty disables escalation.
fn elevator() -> Option<String> {
    let name = match std::env::var("VOW_SUDO") {
        Ok(value) => value.trim().to_string(),
        Err(_) => "sudo".to_string(),
    };
    if name.is_empty() {
        return None;
    }
    which::which(&name).ok().map(|_| name)
}

fn quote(text: &str) -> String {
    let safe = !text.is_empty()
        && text.chars().all(|c| {
            c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '=' | '@' | ':' | '+' | ',')
        });
    if safe {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_reports_what_the_command_complained_about() {
        let inv = Invocation::new("sh").args(["-c", "echo boom >&2; exit 1"]);
        let error = inv.capture().expect_err("must fail");
        assert!(error.to_string().contains("boom"), "got: {error}");
    }

    #[test]
    fn a_captured_run_keeps_both_streams_and_the_complaint() {
        let inv = Invocation::new("sh").args(["-c", "echo out; echo boom >&2; exit 1"]);
        let (transcript, result) = inv.run_captured();
        assert!(transcript.contains("out") && transcript.contains("boom"));
        assert!(result.expect_err("must fail").to_string().contains("boom"));
    }

    #[test]
    fn a_query_that_exits_non_zero_still_answers() {
        let inv = Invocation::new("sh").args(["-c", "echo ripgrep; exit 1"]);
        assert_eq!(inv.capture_any_status().expect("answers"), "ripgrep\n");
    }

    #[test]
    fn capture_returns_stdout_on_success() {
        let inv = Invocation::new("printf").arg("vim 9.1\n");
        assert_eq!(inv.capture().expect("succeeds"), "vim 9.1\n");
    }

    #[test]
    fn display_quotes_only_what_needs_it() {
        let inv = Invocation::new("cargo").args(["install", "ripgrep@14.1.0", "weird name"]);
        assert_eq!(inv.display(), "cargo install ripgrep@14.1.0 'weird name'");
    }

    #[test]
    fn empty_vow_sudo_disables_escalation() {
        temp_env("VOW_SUDO", "", || {
            let inv = Invocation::new("pacman").arg("-S").with_root();
            assert_eq!(inv.resolve().0, "pacman");
        });
    }

    fn temp_env(key: &str, value: &str, body: impl FnOnce()) {
        let previous = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        body();
        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}
