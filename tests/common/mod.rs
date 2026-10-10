//! Shared by the end-to-end tests: a scratch home and graph, and stand-in `cargo`
//! and `apk` that keep what they install in a file.

#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo install --list`, `install`, `uninstall` and `search`, against the
/// file in `$FAKE_CARGO` of `name version` lines. A crate whose name starts
/// with `broken` fails to build, one starting `ghost` says it installed but
/// never does, and once one starting `corrupt` is in, listing fails.
const FAKE_CARGO: &str = r#"#!/bin/sh
state="$FAKE_CARGO"
touch "$state"
drop() { grep -v "^$1 " "$state" > "$state.tmp"; mv "$state.tmp" "$state"; }
case "$1" in
  install)
    shift
    if [ "$1" = "--list" ]; then
      grep -q '^corrupt' "$state" && { echo "error: corrupt metadata" >&2; exit 1; }
      while read -r name version; do
        printf '%s v%s:\n    %s\n' "$name" "$version" "$name"
      done < "$state"
      exit 0
    fi
    [ "$1" = "--force" ] && shift
    for spec in "$@"; do
      name=${spec%@*}
      version=${spec#*@}
      [ "$version" = "$spec" ] && version=1.0.0
      case "$name" in
        broken*) echo "error: could not compile \`$name\`" >&2; exit 101 ;;
        ghost*) echo "  Installed package \`$name\`"; continue ;;
      esac
      drop "$name"
      echo "$name $version" >> "$state"
      echo "  Installed package \`$name v$version\`"
    done ;;
  uninstall)
    shift
    for name in "$@"; do drop "$name"; echo "    Removing $name"; done ;;
  search)
    echo "$2 = \"1.0.0\"    # a crate called $2" ;;
  *)
    echo "fake cargo cannot $*" >&2; exit 2 ;;
esac
"#;

/// `apk info`, `add` and `del`, against the file in `$FAKE_APK`. Adding
/// `cargo` puts the stand-in cargo on `PATH`, as a system manager installing
/// a toolchain does.
const FAKE_APK: &str = r#"#!/bin/sh
state="$FAKE_APK"
touch "$state"
case "$1" in
  info) cat "$state" ;;
  add)
    shift
    for name in "$@"; do
      grep -qx "$name" "$state" || echo "$name" >> "$state"
      if [ "$name" = cargo ]; then cp "$FAKE_BIN/../cargo.sh" "$FAKE_BIN/cargo"; fi
    done ;;
  del)
    shift
    for name in "$@"; do
      grep -vx "$name" "$state" > "$state.tmp"; mv "$state.tmp" "$state"
      if [ "$name" = cargo ]; then rm -f "$FAKE_BIN/cargo"; fi
    done ;;
  *) echo "fake apk cannot $*" >&2; exit 2 ;;
esac
"#;

/// A scratch home holding a graph, removed when the test ends.
pub struct Sandbox {
    pub root: PathBuf,
}

/// What vow printed, both streams, and whether it succeeded.
pub struct Run {
    pub text: String,
    pub ok: bool,
}

impl Sandbox {
    pub fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default();
        let root = std::env::temp_dir().join(format!("vow-e2e-{tag}-{unique}"));
        fs::create_dir_all(root.join("home")).expect("home");
        fs::create_dir_all(root.join("bin")).expect("bin");
        for (path, script) in [
            ("cargo.sh", FAKE_CARGO),
            ("bin/cargo", FAKE_CARGO),
            ("bin/apk", FAKE_APK),
        ] {
            fs::write(root.join(path), script).expect("fake manager");
            fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o755))
                .expect("executable");
        }
        Self { root }
    }

    /// Take the stand-in cargo off `PATH`, for apk to install.
    pub fn without_cargo(&self) {
        fs::remove_file(self.root.join("bin/cargo")).expect("cargo");
    }

    pub fn graph(&self) -> PathBuf {
        self.root.join("graph")
    }

    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// Write `text` at `path` in the graph, making its directory first.
    pub fn write(&self, path: &str, text: &str) {
        let path = self.graph().join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        fs::write(path, text).expect("write");
    }

    /// What the stand-in cargo has installed, as `name version` lines sorted.
    pub fn crates(&self) -> Vec<String> {
        let text = fs::read_to_string(self.root.join("cargo-state")).unwrap_or_default();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines.sort();
        lines
    }

    /// Have the stand-in cargo hold exactly these `name version` lines.
    pub fn install(&self, crates: &[&str]) {
        let text: String = crates.iter().map(|line| format!("{line}\n")).collect();
        fs::write(self.root.join("cargo-state"), text).expect("state");
    }

    /// What the record says vow placed on this machine.
    pub fn record(&self) -> String {
        fs::read_to_string(self.home().join(".local/share/vow/links")).unwrap_or_default()
    }

    /// Run vow on this graph as the host `box`.
    pub fn vow(&self, args: &[&str]) -> Run {
        self.vow_on(&self.graph(), "box", args)
    }

    /// Run vow on another graph, or as another host.
    pub fn vow_on(&self, graph: &Path, host: &str, args: &[&str]) -> Run {
        let path = format!("{}:/usr/bin:/bin", self.root.join("bin").display());
        let output = Command::new(env!("CARGO_BIN_EXE_vow"))
            .args(args)
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.home())
            .env("VOW_CONFIG_DIR", graph)
            .env("VOW_HOST", host)
            .env("VOW_SUDO", "")
            .env("NO_COLOR", "1")
            .env("FAKE_CARGO", self.root.join("cargo-state"))
            .env("FAKE_APK", self.root.join("apk-state"))
            .env("FAKE_BIN", self.root.join("bin"))
            .output()
            .expect("vow runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            text,
            ok: output.status.success(),
        }
    }

    /// Run vow, which must succeed, and return what it printed.
    pub fn ok(&self, args: &[&str]) -> String {
        let run = self.vow(args);
        assert!(run.ok, "vow {args:?} failed:\n{}", run.text);
        run.text
    }

    /// Run vow, which must fail, and return what it printed.
    pub fn fails(&self, args: &[&str]) -> String {
        let run = self.vow(args);
        assert!(!run.ok, "vow {args:?} succeeded:\n{}", run.text);
        run.text
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// A `[links]` entry.
pub fn link(name: &str, source: &str, target: &str) -> String {
    format!("{name} = {{ source = \"{source}\", target = \"{target}\" }}\n")
}
