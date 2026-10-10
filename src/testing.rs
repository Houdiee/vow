//! What the unit tests share: a throwaway directory, and a tree read from it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::graph::{Graph, Tree};

/// A directory removed when the test ends.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("vow-{tag}-{unique}"));
        fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Write `body` to `path`, making its directory first.
pub fn seed(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
}

/// The tree `config.toml` heads in `dir`, after writing each `(path, text)`.
pub fn tree(dir: &TempDir, files: &[(&str, &str)]) -> Tree {
    for (path, text) in files {
        seed(&dir.join(path), text);
    }
    let graph = Graph::at(&dir.0);
    let tree = Tree::load(&graph, &graph.root().join("config.toml"));
    tree.check().expect("reads");
    tree
}
