//! Where vow keeps what it knows about this machine: outside the graph, which
//! may be shared between machines.

use std::path::PathBuf;

/// `~/.local/share/vow`: the one place vow keeps state.
pub fn home() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".local/share/vow"))
}
