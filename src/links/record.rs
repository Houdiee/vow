//! The links `switch` placed on this machine, in `~/.local/share/vow/links`.
//! A link stays vow's while it points where the record says.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::Placement;
use crate::atomic;
use crate::state;

#[derive(Debug, Default)]
pub struct Record(BTreeMap<PathBuf, Placement>);

impl Record {
    /// A missing or unreadable record is empty: vow then owns no links.
    pub fn load() -> Self {
        let text = place()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        Self(
            text.lines()
                .filter_map(parse)
                .map(|link| (link.target.clone(), link))
                .collect(),
        )
    }

    pub fn save(links: &[Placement]) -> Result<()> {
        let path = place().context("could not locate the state directory")?;
        let mut text = String::from("# links vow placed on this machine\n");
        for link in links {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                link.file.display(),
                link.target.display(),
                link.entry.display(),
                link.source.display()
            ));
        }
        atomic::write(&path, &text)
    }

    /// The link vow placed at `target`, if any.
    pub fn get(&self, target: &Path) -> Option<&Placement> {
        self.0.get(target)
    }

    pub fn links(&self) -> impl Iterator<Item = &Placement> {
        self.0.values()
    }
}

#[cfg(test)]
impl From<&[Placement]> for Record {
    fn from(links: &[Placement]) -> Self {
        Self(
            links
                .iter()
                .map(|link| (link.target.clone(), link.clone()))
                .collect(),
        )
    }
}

fn parse(line: &str) -> Option<Placement> {
    if line.starts_with('#') {
        return None;
    }
    let mut fields = line.split('\t');
    Some(Placement {
        file: PathBuf::from(fields.next()?),
        target: PathBuf::from(fields.next()?),
        entry: PathBuf::from(fields.next()?),
        source: PathBuf::from(fields.next()?),
    })
}

fn place() -> Option<PathBuf> {
    Some(state::home()?.join("links"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_reads_back_what_was_saved() {
        let link = Placement {
            source: PathBuf::from("/graph/dotfiles/nvim"),
            entry: PathBuf::from("dotfiles/nvim"),
            target: PathBuf::from("/home/me/.config/nvim"),
            file: PathBuf::from("roles/dev.toml"),
        };
        let text = format!(
            "# header\n{}\t{}\t{}\t{}\n",
            link.file.display(),
            link.target.display(),
            link.entry.display(),
            link.source.display()
        );
        let parsed: Vec<Placement> = text.lines().filter_map(parse).collect();
        assert_eq!(parsed, vec![link]);
    }
}
