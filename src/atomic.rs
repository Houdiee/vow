//! Writes that happen whole or not at all: each is built beside its
//! destination and renamed over it, which the OS does in one step. An
//! interrupted run leaves the old file or link, never half of a new one.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::report;

/// Replace `path` with `text`.
pub fn write(path: &Path, text: &str) -> Result<()> {
    let temp = beside(path)?;
    let mut file = fs::File::create(&temp)
        .with_context(|| format!("could not create {}", report::path(&temp)))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("could not write {}", report::path(&temp)))?;
    rename(&temp, path)
}

/// Make `link` a symlink to `source`, replacing a symlink already there.
pub fn symlink(source: &Path, link: &Path) -> Result<()> {
    let temp = beside(link)?;
    let _ = fs::remove_file(&temp);
    make_symlink(source, &temp)
        .with_context(|| format!("could not link {}", report::path(&temp)))?;
    rename(&temp, link).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// A hidden temporary path in `path`'s directory, which is created if missing.
fn beside(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", report::path(path)))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("could not create {}", report::path(parent)))?;
    let name = path
        .file_name()
        .with_context(|| format!("{} has no file name", report::path(path)))?;
    Ok(parent.join(format!(".{}.vow-tmp", name.to_string_lossy())))
}

fn rename(temp: &Path, path: &Path) -> Result<()> {
    fs::rename(temp, path).with_context(|| format!("could not replace {}", report::path(path)))
}

#[cfg(unix)]
fn make_symlink(source: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, link)
}

#[cfg(not(unix))]
fn make_symlink(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("symlinks are only supported on unix"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    #[test]
    fn a_write_replaces_the_file_and_leaves_nothing_behind() {
        let dir = TempDir::new("atomic");
        let path = dir.join("nested/file");
        write(&path, "one\n").expect("writes");
        write(&path, "two\n").expect("replaces");
        assert_eq!(fs::read_to_string(&path).expect("read"), "two\n");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn a_symlink_replaces_a_symlink() {
        let dir = TempDir::new("atomic-link");
        let link = dir.join("link");
        symlink(&dir.join("a"), &link).expect("links");
        symlink(&dir.join("b"), &link).expect("relinks");
        assert_eq!(fs::read_link(&link).expect("a link"), dir.join("b"));
    }
}
