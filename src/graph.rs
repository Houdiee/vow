//! The graph: the folder of TOML files machines declare in, and the [`Tree`] of
//! files one machine applies: `--file`, else its host file in `hosts/`, else `config.toml`.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use toml::Spanned;

use crate::diagnostic::{Diagnostic, Diagnostics, Source, Span};
use crate::report;

/// The file a machine applies when no host file names it.
pub const ENTRY: &str = "config.toml";
/// Where host files live: `hosts/<name>.toml` or `hosts/<name>/<name>.toml`.
pub const HOSTS: &str = "hosts";

#[derive(Debug, Clone, Default)]
pub struct Graph {
    root: PathBuf,
}

impl Graph {
    /// `--path` if given, else `$VOW_CONFIG_DIR`, else `$XDG_CONFIG_HOME/vow`
    /// or `~/.config/vow`.
    pub fn discover(explicit: Option<&Path>) -> Result<Self> {
        // Absolute, since links placed from it must point here from anywhere.
        let root = match explicit {
            Some(path) => path.to_path_buf(),
            None => match std::env::var("VOW_CONFIG_DIR") {
                Ok(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
                _ => config_home()?.join("vow"),
            },
        };
        // Where it really is, so a graph reached through a symlink doesn't look moved.
        let root = std::path::absolute(&root)
            .with_context(|| format!("could not resolve {}", report::path(&root)))?;
        Ok(Self { root: real(&root) })
    }

    #[cfg(test)]
    pub fn at(root: impl AsRef<Path>) -> Self {
        Self {
            root: real(root.as_ref()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Refuse a missing graph: a typo or an unmounted disk would read as declaring
    /// nothing, and a switch would remove every link it placed.
    pub fn check(&self) -> Result<()> {
        if !self.root.is_dir() {
            bail!(
                "there is no graph at {}; `vow generate-config` starts one, or pass \
                 `--path` to use another",
                report::path(&self.root)
            );
        }
        Ok(())
    }

    /// `path` if inside the graph, judged by where it really leads through symlinks.
    pub fn within(&self, path: &Path) -> Option<PathBuf> {
        let path = normalize(path);
        if !real(&path).starts_with(&self.root) {
            return None;
        }
        Some(if path.starts_with(&self.root) {
            path
        } else {
            real(&path)
        })
    }

    /// A file's path in the graph, such as `hosts/laptop.toml`.
    pub fn name(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(inside) => inside.display().to_string(),
            Err(_) => report::path(path),
        }
    }

    /// The hosts this graph has a file for, by name.
    pub fn hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = fs::read_dir(self.root.join(HOSTS))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_string();
                let host = name.strip_suffix(".toml").unwrap_or(&name).to_string();
                (!self.host_files(&host).is_empty()).then_some(host)
            })
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    /// `hosts/<host>.toml` and `hosts/<host>/<host>.toml`, where they exist.
    fn host_files(&self, host: &str) -> Vec<PathBuf> {
        let hosts = self.root.join(HOSTS);
        [
            hosts.join(format!("{host}.toml")),
            hosts.join(host).join(format!("{host}.toml")),
        ]
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
    }

    /// `host`'s file; both spellings at once is refused, as either could be meant.
    fn host_file(&self, host: &str) -> Result<Option<PathBuf>> {
        if host.is_empty() || host.contains('/') || host.starts_with('.') {
            bail!("`{host}` is not a host name");
        }
        match self.host_files(host).as_slice() {
            [] => Ok(None),
            [path] => Ok(Some(path.clone())),
            [one, other, ..] => bail!(
                "both {} and {} are `{host}`'s file; keep one",
                report::path(one),
                report::path(other)
            ),
        }
    }

    /// The file a machine applies: `file`, else `host`'s, else this machine's, else
    /// `config.toml`. A named host must have a file; this machine needs none.
    pub fn entry(&self, file: Option<&Path>, host: Option<&str>) -> Result<PathBuf> {
        if let Some(file) = file {
            let path = self.file(file)?;
            if !path.is_file() {
                bail!("there is no file at {}", report::path(&path));
            }
            return Ok(real(&path));
        }
        let Some(host) = host else {
            let path = match self.this_host()? {
                Some((_, path)) => path,
                None => self.root.join(ENTRY),
            };
            return Ok(real(&path));
        };
        if let Some(path) = self.host_file(host)? {
            return Ok(real(&path));
        }
        let hosts = self.hosts();
        let known = if hosts.is_empty() {
            format!("{} has no host files", report::path(&self.root.join(HOSTS)))
        } else {
            format!("the hosts are {}", hosts.join(", "))
        };
        bail!(
            "no host `{host}`: there is no `{HOSTS}/{host}.toml` or \
             `{HOSTS}/{host}/{host}.toml`; {known}"
        )
    }

    /// `file`, relative to the graph, refused outside it.
    pub fn file(&self, file: &Path) -> Result<PathBuf> {
        match self.within(&self.root.join(file)) {
            Some(path) => Ok(path),
            None => bail!(
                "{} is outside the graph, {}",
                report::path(&self.root.join(file)),
                report::path(&self.root)
            ),
        }
    }

    /// This machine's host file, by hostname, else by its first part in any case:
    /// macOS answers `Name.local`.
    pub fn this_host(&self) -> Result<Option<(String, PathBuf)>> {
        let this = hostname()?;
        if let Some(path) = self.host_file(&this)? {
            return Ok(Some((this, path)));
        }
        let short = this.split('.').next().unwrap_or(&this);
        let Some(host) = self
            .hosts()
            .into_iter()
            .find(|host| host.eq_ignore_ascii_case(&this) || host.eq_ignore_ascii_case(short))
        else {
            return Ok(None);
        };
        Ok(self.host_file(&host)?.map(|path| (host, path)))
    }

    /// Every file some machine applies: `config.toml` and each host file.
    pub fn entries(&self) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = self
            .hosts()
            .iter()
            .flat_map(|host| self.host_files(host))
            .collect();
        let default = self.root.join(ENTRY);
        if default.is_file() {
            entries.push(default);
        }
        entries.iter().map(|entry| real(entry)).collect()
    }

    /// Every link source any machine's files declare, wherever it applies.
    pub fn sources(&self) -> Vec<PathBuf> {
        let mut sources = Vec::new();
        for entry in self.entries() {
            for file in Tree::load(self, &entry).files {
                for decl in file.doc.links.values() {
                    if let Ok(source) = resolve_inside(self, file.dir(), decl.source.get_ref()) {
                        sources.push(source);
                    }
                }
            }
        }
        sources
    }

    /// Files that read as vow's but that neither `applied` nor any machine's file
    /// reaches. A tool's own TOML doesn't read as vow's, so is left alone.
    pub fn warnings(&self, applied: &Tree) -> Diagnostics {
        let mut warnings = Diagnostics::default();
        let mut reached: Vec<PathBuf> =
            applied.files.iter().map(|file| file.path.clone()).collect();
        for entry in self.entries() {
            let tree = Tree::load(self, &entry);
            reached.extend(tree.files.iter().map(|file| file.path.clone()));
        }
        for path in toml_files(&self.root) {
            if reached.contains(&path) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if toml::from_str::<Document>(&text).is_ok_and(|doc| !doc.is_empty()) {
                warnings.push(
                    Diagnostic::warning(format!(
                        "`{}` is included by no host file or `{ENTRY}`",
                        self.name(&path)
                    ))
                    .in_place(&path)
                    .note("include it from a file a machine applies, or remove it"),
                );
            }
        }
        warnings
    }
}

/// One file as written, every value keeping its place for faults found later.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    #[serde(default)]
    pub includes: Vec<Spanned<String>>,
    /// Manager name to its packages, each `name` or `name version`.
    #[serde(default)]
    pub packages: BTreeMap<Spanned<String>, Vec<Spanned<String>>>,
    /// Link name to where it comes from and goes.
    #[serde(default)]
    pub links: BTreeMap<Spanned<String>, LinkDecl>,
}

impl Document {
    fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.packages.is_empty() && self.links.is_empty()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkDecl {
    /// The file or directory in the graph, relative to the file declaring it.
    pub source: Spanned<String>,
    /// Where the symlink goes: an absolute path, or one starting with `~`.
    pub target: Spanned<String>,
}

/// A file read, with its text for faults to quote.
#[derive(Debug)]
pub struct File {
    /// Canonical, so one file reached two ways is read once.
    pub path: PathBuf,
    pub source: Source,
    pub doc: Document,
}

impl File {
    /// The folder its paths are relative to.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("/"))
    }
}

/// The file a machine applies and every file it includes, each read once, entry
/// first. A file that could not be read is a fault, said by [`Tree::check`].
#[derive(Debug, Default)]
pub struct Tree {
    pub graph: Graph,
    pub entry: PathBuf,
    pub files: Vec<File>,
    problems: Diagnostics,
}

impl Tree {
    /// Read `entry` and what it includes. A missing `entry` declares nothing.
    pub fn load(graph: &Graph, entry: &Path) -> Self {
        let mut tree = Self {
            graph: graph.clone(),
            entry: entry.to_path_buf(),
            ..Self::default()
        };
        if entry.exists() {
            tree.read(graph, entry, &mut Vec::new(), None);
        }
        tree
    }

    /// Refuse a tree with a file that could not be read or included.
    pub fn check(&self) -> Result<()> {
        self.problems.clone().into_result()
    }

    /// How output names one of its files: by its path in the graph.
    pub fn name(&self, path: &Path) -> String {
        self.graph.name(path)
    }

    pub fn exists(&self) -> bool {
        !self.files.is_empty()
    }

    /// Files that declare nothing, which is probably a mistake.
    pub fn warnings(&self, graph: &Graph) -> Diagnostics {
        let mut warnings = Diagnostics::default();
        for file in self.files.iter().filter(|file| file.doc.is_empty()) {
            warnings.push(
                Diagnostic::warning(format!("`{}` declares nothing", graph.name(&file.path)))
                    .in_place(&file.path)
                    .note("give it `includes`, `[packages]` or `[links]`, or remove it"),
            );
        }
        warnings
    }

    /// Read `path` unless it was read already; `chain` is the includes that led
    /// here, and `by` the include naming it, where a fault is drawn.
    fn read(
        &mut self,
        graph: &Graph,
        path: &Path,
        chain: &mut Vec<PathBuf>,
        by: Option<(&Source, Span)>,
    ) {
        let fault = |diagnostic: Diagnostic, label: &str| match &by {
            Some((source, span)) => source.fault(diagnostic.at(span.clone(), label.to_string())),
            None => diagnostic.in_place(path),
        };
        if let Some(at) = chain.iter().position(|seen| seen == path) {
            let mut names: Vec<String> = chain[at..].iter().map(|seen| graph.name(seen)).collect();
            names.push(graph.name(path));
            self.problems.push(
                fault(
                    Diagnostic::error(format!(
                        "`{}` includes itself: {}",
                        graph.name(path),
                        names.join(" → ")
                    )),
                    "included again here",
                )
                .note("a file can only include what does not include it back"),
            );
            return;
        }
        if self.files.iter().any(|file| file.path == path) {
            return;
        }

        let text: Arc<str> = match fs::read_to_string(path) {
            Ok(text) => text.into(),
            Err(error) => {
                self.problems.push(fault(
                    Diagnostic::error(format!("could not read `{}`: {error}", graph.name(path))),
                    "included here",
                ));
                return;
            }
        };
        let source = Source::new(path, &text);
        let doc = match toml::from_str::<Document>(&text) {
            Ok(doc) => doc,
            Err(error) => {
                let problem = Diagnostic::error(error.message().trim().to_string());
                self.problems.push(source.fault(match error.span() {
                    Some(span) => problem.at(span, "here"),
                    None => problem,
                }));
                return;
            }
        };

        let includes: Vec<Spanned<String>> = doc.includes.clone();
        self.files.push(File {
            path: path.to_path_buf(),
            source: source.clone(),
            doc,
        });
        chain.push(path.to_path_buf());
        let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        for include in &includes {
            match resolve_inside(graph, &dir, include.get_ref()) {
                Ok(included) if included.is_file() => {
                    self.read(
                        graph,
                        &real(&included),
                        chain,
                        Some((&source, include.span())),
                    );
                }
                Ok(included) => self.problems.push(
                    source.fault(
                        Diagnostic::error(format!(
                            "there is no file at `{}`",
                            graph.name(&included)
                        ))
                        .at(include.span(), "included here")
                        .note("a path in `includes` is relative to the file naming it"),
                    ),
                ),
                Err(problem) => {
                    self.problems
                        .push(source.fault(problem.at(include.span(), "included here")));
                }
            }
        }
        chain.pop();
    }
}

/// `path` as written in a file in `dir`, refused outside the graph.
pub fn resolve_inside(graph: &Graph, dir: &Path, path: &str) -> Result<PathBuf, Diagnostic> {
    if path.contains(char::is_control) {
        return Err(
            Diagnostic::error("a path cannot contain a control character").note(
                "rename the file, or write the path without tabs or other control characters",
            ),
        );
    }
    graph.within(&dir.join(path)).ok_or_else(|| {
        Diagnostic::error(format!("`{path}` is outside the graph")).note(format!(
            "paths are relative to the file naming them, and stay inside {}",
            report::path(graph.root())
        ))
    })
}

/// Where `path` really leads: its deepest existing part with every symlink
/// resolved, then the rest as written.
fn real(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(resolved) = fs::canonicalize(ancestor) {
            let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return if rest.as_os_str().is_empty() {
                resolved
            } else {
                resolved.join(rest)
            };
        }
    }
    path.to_path_buf()
}

/// `path` with `.` and `..` worked out, without asking the file system.
pub fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

/// Every `.toml` file under `dir`, not following symlinks or entering `.git`.
fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && entry.file_name() != ".git" {
            found.extend(toml_files(&path));
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "toml") {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Entries of `dir` in name order, dotfiles included, but not `.git` or vow's
/// temporary files.
pub fn entries_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| name != ".git" && !name.ends_with(".vow-tmp"))
        .collect();
    names.sort();
    names
}

/// This machine's name: an error rather than a guess, which would apply another
/// machine's file.
pub fn hostname() -> Result<String> {
    let candidates = [
        std::env::var("VOW_HOST").ok(),
        fs::read_to_string("/etc/hostname").ok(),
        std::process::Command::new("hostname")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned()),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .context("could not determine this machine's hostname; set VOW_HOST to name it")
}

/// `$XDG_CONFIG_HOME`, else `~/.config` -- on macOS too, where
/// `dirs::config_dir()` would answer `~/Library/Application Support`.
fn config_home() -> Result<PathBuf> {
    if let Some(configured) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(configured);
        if path.is_absolute() {
            return Ok(path);
        }
    }
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join(".config"));
    }
    dirs::config_dir()
        .context("could not find where to keep the graph; pass `--path` or set VOW_CONFIG_DIR")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TempDir, seed};

    fn graph(dir: &TempDir) -> Graph {
        Graph::at(&dir.0)
    }

    fn names(graph: &Graph, tree: &Tree) -> Vec<String> {
        tree.files
            .iter()
            .map(|file| graph.name(&file.path))
            .collect()
    }

    #[test]
    fn includes_are_read_once_relative_to_the_file_naming_them() {
        let dir = TempDir::new("includes");
        let graph = graph(&dir);
        seed(
            &dir.join("hosts/box.toml"),
            "includes = [\"../roles/rust.toml\", \"../config.toml\"]\n",
        );
        seed(
            &dir.join("roles/rust.toml"),
            "includes = [\"../config.toml\"]\n",
        );
        seed(&dir.join("config.toml"), "[packages]\ncargo = []\n");

        let tree = Tree::load(&graph, &dir.join("hosts/box.toml"));
        tree.check().expect("reads");
        assert_eq!(
            names(&graph, &tree),
            ["hosts/box.toml", "roles/rust.toml", "config.toml"]
        );
    }
}
