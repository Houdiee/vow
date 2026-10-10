//! The links half end to end: however a link stops applying, the next switch
//! removes its symlink, and only one vow placed.

mod common;

use common::{Sandbox, is_link, link};
use std::fs;
use std::path::{Path, PathBuf};

/// The links half only, which these tests are about.
trait Links {
    /// Run vow on the links half, which must succeed.
    fn links(&self, args: &[&str]) -> String;
    /// The same, on another graph or as another host.
    fn links_on(&self, graph: &Path, host: &str, args: &[&str]) -> String;
    /// Switch `~/.config/<name>` into place, check it is linked and recorded.
    fn placed(&self, name: &str) -> PathBuf;
    /// The same, for a target anywhere.
    fn placed_at(&self, target: &Path) -> PathBuf;
    /// The link is gone after the change: planned as a removal, removed by a
    /// switch, dropped from the record, and nothing left to do.
    fn removes(&self, target: &Path, entry: &str);
}

impl Links for Sandbox {
    fn links(&self, args: &[&str]) -> String {
        self.links_on(&self.graph(), "box", args)
    }

    fn links_on(&self, graph: &Path, host: &str, args: &[&str]) -> String {
        let args: Vec<&str> = args.iter().copied().chain(["--links"]).collect();
        let run = self.vow_on(graph, host, &args);
        assert!(run.ok, "vow {args:?} failed:\n{}", run.text);
        run.text
    }

    fn placed(&self, name: &str) -> PathBuf {
        self.placed_at(&self.home().join(".config").join(name))
    }

    fn placed_at(&self, target: &Path) -> PathBuf {
        self.links(&["switch", "--yes"]);
        assert!(is_link(target), "{} was not linked", target.display());
        assert!(
            self.record().contains(&target.display().to_string()),
            "not recorded:\n{}",
            self.record()
        );
        target.to_path_buf()
    }

    fn removes(&self, target: &Path, entry: &str) {
        let status = self.links(&["status"]);
        assert!(
            status.contains(&format!("- {entry}")),
            "status does not plan the removal:\n{status}"
        );
        let switch = self.links(&["switch", "--yes"]);
        assert!(!is_link(target), "still linked after:\n{switch}");
        assert!(
            !self.record().contains(&target.display().to_string()),
            "still recorded:\n{}",
            self.record()
        );
        let again = self.links(&["switch", "--yes"]);
        assert!(
            again.contains("nothing to change")
                || again.contains("no links declared")
                || again.contains("declared yet"),
            "{again}"
        );
    }
}

/// `config.toml` linking `dotfiles/nvim` and `dotfiles/git`, or only the first
/// `count` of them.
fn links(count: usize) -> String {
    let all = [
        link("nvim", "dotfiles/nvim", "~/.config/nvim"),
        link("git", "dotfiles/git", "~/.config/git"),
    ];
    format!("[links]\n{}", all[..count].concat())
}

/// A graph whose `config.toml` links `dotfiles/nvim` and `dotfiles/git`.
fn two_links(tag: &str) -> Sandbox {
    let sandbox = Sandbox::new(tag);
    sandbox.write("dotfiles/nvim/init.lua", "");
    sandbox.write("dotfiles/git/config", "");
    sandbox.write("config.toml", &links(2));
    sandbox
}

/// `config.toml` linking only `dotfiles/git`.
fn only_git() -> String {
    format!("[links]\n{}", link("git", "dotfiles/git", "~/.config/git"))
}

#[test]
fn deleting_the_link_removes_the_symlink() {
    let sandbox = two_links("by-hand");
    let target = sandbox.placed("nvim");
    sandbox.write("config.toml", &only_git());
    sandbox.removes(&target, "dotfiles/nvim");
    assert!(is_link(&sandbox.home().join(".config/git")), "git was kept");
}

#[test]
fn deleting_the_whole_file_removes_every_symlink() {
    let sandbox = two_links("no-file");
    let nvim = sandbox.placed("nvim");
    let git = sandbox.home().join(".config/git");
    fs::remove_file(sandbox.graph().join("config.toml")).expect("rm");
    sandbox.removes(&nvim, "dotfiles/nvim");
    assert!(!is_link(&git), "git is linked still");
}

#[test]
fn a_file_no_longer_included_has_its_symlinks_removed() {
    let sandbox = Sandbox::new("include-dropped");
    sandbox.write("roles/tmux.conf", "");
    sandbox.write(
        "roles/dev.toml",
        &format!(
            "[links]\n{}",
            link("tmux", "tmux.conf", "~/.config/tmux.conf")
        ),
    );
    sandbox.write("config.toml", "includes = [\"roles/dev.toml\"]\n");
    let target = sandbox.placed("tmux.conf");
    sandbox.write("config.toml", "includes = []\n");
    sandbox.removes(&target, "roles/tmux.conf");
}

#[test]
fn a_link_whose_source_was_deleted_too_is_removed() {
    let sandbox = two_links("dangling");
    let target = sandbox.placed("nvim");
    fs::remove_dir_all(sandbox.graph().join("dotfiles/nvim")).expect("rm");
    sandbox.write("config.toml", &only_git());
    sandbox.removes(&target, "dotfiles/nvim");
}

#[test]
fn a_link_someone_else_has_repointed_is_left_alone() {
    let sandbox = two_links("repointed");
    let target = sandbox.placed("nvim");
    fs::remove_file(&target).expect("rm");
    std::os::unix::fs::symlink("/somewhere/else", &target).expect("relink");
    sandbox.write("config.toml", &only_git());

    sandbox.links(&["switch", "--yes"]);
    assert_eq!(
        fs::read_link(&target).expect("kept"),
        PathBuf::from("/somewhere/else")
    );
}

#[test]
fn a_file_now_where_the_link_was_is_left_alone() {
    let sandbox = two_links("replaced-by-file");
    let target = sandbox.placed("nvim");
    fs::remove_file(&target).expect("rm");
    fs::write(&target, "mine").expect("write");
    sandbox.write("config.toml", &only_git());

    sandbox.links(&["switch", "--yes"]);
    assert_eq!(fs::read_to_string(&target).expect("kept"), "mine");
}

/// vow's record is the machine's, not the graph's: moving the graph leaves
/// vow's links its own, so they are repointed rather than refused.
#[test]
fn a_moved_graph_repoints_its_links() {
    let sandbox = two_links("moved");
    let target = sandbox.placed("nvim");
    let moved = sandbox.root.join("dotfiles");
    fs::rename(sandbox.graph(), &moved).expect("move");

    let status = sandbox.links_on(&moved, "box", &["status"]);
    assert!(
        status.contains("~ dotfiles/nvim") && status.contains("(graph moved from"),
        "{status}"
    );
    sandbox.links_on(&moved, "box", &["switch", "--yes"]);
    assert_eq!(
        fs::read_link(&target).expect("linked"),
        moved.join("dotfiles/nvim")
    );
    let again = sandbox.links_on(&moved, "box", &["switch", "--yes"]);
    assert!(again.contains("nothing to change"), "{again}");
}

#[test]
fn a_renamed_machine_without_a_host_file_keeps_its_links() {
    let sandbox = two_links("renamed");
    sandbox.placed("nvim");
    let status = sandbox.links_on(&sandbox.graph(), "box-renamed", &["status"]);
    assert!(status.contains("nothing to change"), "{status}");
}

#[test]
fn switching_to_another_graph_removes_the_first_ones_links() {
    let sandbox = two_links("other-graph");
    let nvim = sandbox.placed("nvim");
    let other = sandbox.root.join("other");
    fs::create_dir_all(other.join("git")).expect("dirs");
    fs::write(
        other.join("config.toml"),
        format!("[links]\n{}", link("git", "git", "~/.config/git")),
    )
    .expect("write");

    sandbox.links_on(&other, "box", &["switch", "--yes"]);
    assert!(!is_link(&nvim), "the first graph's nvim link stayed");
    assert_eq!(
        fs::read_link(sandbox.home().join(".config/git")).expect("linked"),
        other.join("git")
    );
}

/// Two hosts sharing a file, each including its own role.
#[test]
fn switching_hosts_swaps_their_links_and_keeps_what_they_share() {
    let sandbox = Sandbox::new("hosts");
    sandbox.write("shell/zshrc", "");
    sandbox.write(
        "shared.toml",
        &format!("[links]\n{}", link("zsh", "shell/zshrc", "~/.zshrc")),
    );
    sandbox.write("dev/nvim/init.lua", "");
    sandbox.write(
        "dev/dev.toml",
        &format!("[links]\n{}", link("nvim", "nvim", "~/.config/nvim")),
    );
    sandbox.write("games/steam.cfg", "");
    sandbox.write(
        "games/games.toml",
        &format!(
            "[links]\n{}",
            link("steam", "steam.cfg", "~/.config/steam.cfg")
        ),
    );
    sandbox.write(
        "hosts/laptop.toml",
        "includes = [\"../shared.toml\", \"../dev/dev.toml\"]\n",
    );
    sandbox.write(
        "hosts/desktop/desktop.toml",
        "includes = [\"../../shared.toml\", \"../../games/games.toml\"]\n",
    );
    let (graph, home) = (sandbox.graph(), sandbox.home());
    let (zshrc, nvim, steam) = (
        home.join(".zshrc"),
        home.join(".config/nvim"),
        home.join(".config/steam.cfg"),
    );

    sandbox.links_on(&graph, "laptop", &["switch", "--yes"]);
    assert!(is_link(&zshrc) && is_link(&nvim) && !is_link(&steam));

    let status = sandbox.links_on(&graph, "desktop", &["status"]);
    assert!(
        status.contains("- dev/nvim") && status.contains("+ games/steam.cfg"),
        "{status}"
    );
    assert!(!status.contains("zshrc"), "shared is untouched:\n{status}");
    sandbox.links_on(&graph, "desktop", &["switch", "--yes"]);
    assert!(is_link(&zshrc) && !is_link(&nvim) && is_link(&steam));

    // Named instead of found by this machine's name.
    sandbox.links_on(&graph, "anything", &["--host", "laptop", "switch", "--yes"]);
    assert!(is_link(&zshrc) && is_link(&nvim) && !is_link(&steam));
}

#[test]
fn a_link_moved_to_another_file_or_renamed_is_the_same_link() {
    let sandbox = Sandbox::new("to-host");
    sandbox.write("gitconfig", "");
    sandbox.write(
        "config.toml",
        &format!("[links]\n{}", link("git", "gitconfig", "~/.gitconfig")),
    );
    let target = sandbox.placed_at(&sandbox.home().join(".gitconfig"));

    sandbox.write("config.toml", "");
    sandbox.write(
        "hosts/box.toml",
        &format!(
            "includes = [\"../config.toml\"]\n[links]\n{}",
            link("gitconfig", "../gitconfig", "~/.gitconfig")
        ),
    );
    let status = sandbox.links(&["status"]);
    assert!(status.contains("nothing to change"), "{status}");
    sandbox.links(&["switch", "--yes"]);
    assert_eq!(
        fs::read_link(&target).expect("linked"),
        sandbox.graph().join("gitconfig")
    );
    assert!(
        sandbox
            .record()
            .starts_with("# links vow placed on this machine\nhosts/box.toml\t")
    );
}

#[test]
fn another_source_is_a_different_link() {
    let sandbox = two_links("renamed-entry");
    let target = sandbox.placed("nvim");
    sandbox.write("dotfiles/nvim-lazy/init.lua", "");
    sandbox.write(
        "config.toml",
        &format!(
            "[links]\n{}{}",
            link("nvim", "dotfiles/nvim-lazy", "~/.config/nvim"),
            link("git", "dotfiles/git", "~/.config/git")
        ),
    );

    let status = sandbox.links(&["status"]);
    assert!(
        status.contains("- dotfiles/nvim ") && status.contains("+ dotfiles/nvim-lazy"),
        "{status}"
    );
    assert!(
        !status.contains("~ dotfiles"),
        "not a modification:\n{status}"
    );
    sandbox.links(&["switch", "--yes"]);
    assert_eq!(
        fs::read_link(&target).expect("linked"),
        sandbox.graph().join("dotfiles/nvim-lazy")
    );
    assert!(
        !sandbox.record().contains("\tdotfiles/nvim\t"),
        "{}",
        sandbox.record()
    );
}

#[test]
fn a_lost_record_is_rebuilt_by_a_switch_with_nothing_to_change() {
    let sandbox = two_links("lost-record");
    let target = sandbox.placed("nvim");
    fs::remove_file(sandbox.home().join(".local/share/vow/links")).expect("rm");

    let again = sandbox.links(&["switch", "--yes"]);
    assert!(again.contains("nothing to change"), "{again}");
    assert!(sandbox.record().contains(&target.display().to_string()));
    sandbox.write("config.toml", &only_git());
    sandbox.removes(&target, "dotfiles/nvim");
}

#[test]
fn a_graph_reached_through_a_symlink_has_not_moved() {
    let sandbox = two_links("graph-symlink");
    sandbox.placed("nvim");
    let alias = sandbox.root.join("alias");
    std::os::unix::fs::symlink(sandbox.graph(), &alias).expect("symlink");
    let status = sandbox.links_on(&alias, "box", &["status"]);
    assert!(status.contains("nothing to change"), "{status}");
}

#[test]
fn links_lists_removals_when_nothing_is_declared() {
    let sandbox = two_links("links-removals");
    sandbox.placed("nvim");
    sandbox.write("config.toml", &links(0));
    let listed = sandbox.links(&["links"]);
    assert!(listed.contains("- dotfiles/nvim"), "{listed}");
}

#[test]
fn a_directory_link_splits_into_file_links_and_back() {
    let sandbox = two_links("split");
    let nvim = sandbox.placed("nvim");
    let init = nvim.join("init.lua");
    sandbox.write(
        "config.toml",
        &format!(
            "[links]\n{}{}",
            link("init", "dotfiles/nvim/init.lua", "~/.config/nvim/init.lua"),
            link("git", "dotfiles/git", "~/.config/git")
        ),
    );

    let status = sandbox.links(&["status"]);
    assert!(
        status.contains("- dotfiles/nvim ") && status.contains("+ dotfiles/nvim/init.lua"),
        "{status}"
    );
    sandbox.links(&["switch", "--yes"]);
    assert!(!is_link(&nvim) && is_link(&init));

    sandbox.write("config.toml", &links(2));
    sandbox.links(&["switch", "--yes"]);
    assert_eq!(
        fs::read_link(&nvim).expect("linked"),
        sandbox.graph().join("dotfiles/nvim")
    );
    assert!(sandbox.graph().join("dotfiles/nvim/init.lua").is_file());
    assert!(sandbox.links(&["status"]).contains("nothing to change"));
}

#[test]
fn a_target_with_a_trailing_slash_is_placed_and_kept() {
    let sandbox = Sandbox::new("trailing-slash");
    sandbox.write("dotfiles/git/config", "");
    sandbox.write(
        "config.toml",
        &format!("[links]\n{}", link("git", "dotfiles/git", "~/.config/git/")),
    );
    let target = sandbox.placed("git");
    assert_eq!(
        fs::read_link(&target).expect("linked"),
        sandbox.graph().join("dotfiles/git")
    );
    assert!(sandbox.links(&["status"]).contains("nothing to change"));
    assert!(!sandbox.home().join(".config/.git.vow-tmp").exists());
}

/// A link made by hand that matches the graph must not become vow's to remove.
#[test]
fn a_dry_run_records_nothing() {
    let sandbox = two_links("dry-run");
    for name in ["nvim", "git"] {
        let target = sandbox.home().join(".config").join(name);
        fs::create_dir_all(target.parent().expect("parent")).expect("dirs");
        std::os::unix::fs::symlink(sandbox.graph().join("dotfiles").join(name), &target)
            .expect("by hand");
    }
    let dry = sandbox.links(&["switch", "--dry-run"]);
    assert!(dry.contains("nothing to change"), "{dry}");
    assert!(sandbox.record().is_empty(), "{}", sandbox.record());
}

#[test]
fn a_source_leading_outside_the_graph_is_refused() {
    let sandbox = Sandbox::new("source-outside");
    fs::create_dir_all(sandbox.graph()).expect("graph");
    std::os::unix::fs::symlink("/etc", sandbox.graph().join("etc")).expect("symlink");
    sandbox.write(
        "config.toml",
        &format!("[links]\n{}", link("etc", "etc", "~/.etc")),
    );
    let refused = sandbox.vow(&["status", "--links"]);
    assert!(
        !refused.ok && refused.text.contains("`etc` is outside the graph"),
        "{}",
        refused.text
    );
}
