//! Every command end to end: a scratch home and graph, and stand-in managers
//! that really install and remove.

mod common;

use common::{Sandbox, is_link, link};

/// A graph whose `config.toml` declares `text`.
fn declaring(tag: &str, text: &str) -> Sandbox {
    let sandbox = Sandbox::new(tag);
    sandbox.write("config.toml", text);
    sandbox
}

#[test]
fn a_missing_graph_is_refused_and_an_empty_one_says_how_to_start() {
    let sandbox = Sandbox::new("empty");
    // A typo in `--path` or a disk not mounted must not read as declaring
    // nothing, or a switch would remove every link vow placed.
    for command in ["status", "switch", "links", "managed"] {
        let text = sandbox.fails(&[command]);
        assert!(text.contains("there is no graph at"), "{command}:\n{text}");
    }

    std::fs::create_dir_all(sandbox.graph()).expect("graph");
    for command in ["status", "switch", "outdated", "managed"] {
        let text = sandbox.ok(&[command]);
        assert!(
            text.contains("nothing declared yet") && text.contains("vow generate-config"),
            "{command}:\n{text}"
        );
    }
    assert!(sandbox.ok(&["links"]).contains("no links declared"));
}

#[test]
fn generate_config_declares_what_is_installed_and_never_overwrites() {
    let sandbox = Sandbox::new("generate");
    sandbox.install(&["ripgrep 14.1.0", "bat 0.24.0"]);

    let text = sandbox.ok(&["cargo", "generate-config"]);
    assert!(text.contains("cargo 2 package(s)"), "{text}");
    let written = std::fs::read_to_string(sandbox.graph().join("config.toml")).expect("written");
    assert!(
        written.contains("cargo = [\n  \"bat\",\n  \"ripgrep\",\n]"),
        "{written}"
    );
    assert!(written.contains("[links]"), "{written}");
    assert!(sandbox.ok(&["status"]).contains("nothing to change"));

    let refused = sandbox.fails(&["cargo", "generate-config"]);
    assert!(refused.contains("already exists"), "{refused}");
    assert_eq!(
        std::fs::read_to_string(sandbox.graph().join("config.toml")).expect("kept"),
        written
    );

    // Pinned, to another file, which this machine does not apply.
    let text = sandbox.ok(&["cargo", "generate-config", "--pin", "--file=pinned.toml"]);
    assert!(text.contains("does not include it yet"), "{text}");
    let pinned = std::fs::read_to_string(sandbox.graph().join("pinned.toml")).expect("written");
    assert!(pinned.contains("\"ripgrep 14.1.0\""), "{pinned}");
    let text = sandbox.ok(&["--file=pinned.toml", "status"]);
    assert!(text.contains("nothing to change"), "{text}");
}

#[test]
fn switch_installs_removes_and_repins_what_the_list_says() {
    let sandbox = declaring(
        "switch",
        "[packages]\ncargo = [\"ripgrep\", \"fd 9.0.0\"]\n",
    );
    sandbox.install(&["bat 0.24.0"]);

    let status = sandbox.ok(&["status"]);
    for row in [
        "+ fd 9.0.0",
        "+ ripgrep",
        "- bat",
        "2 to install, 1 to remove",
    ] {
        assert!(status.contains(row), "missing `{row}`:\n{status}");
    }

    // Neither a dry run nor an unanswered prompt changes anything.
    let dry = sandbox.ok(&["switch", "--dry-run"]);
    assert!(dry.contains("dry run: nothing changed"), "{dry}");
    // Nobody to ask is not a yes, nor a quiet success.
    let unanswered = sandbox.fails(&["switch"]);
    assert!(unanswered.contains("pass `--yes`"), "{unanswered}");
    assert_eq!(sandbox.crates(), ["bat 0.24.0"]);

    let switched = sandbox.ok(&["switch", "--yes"]);
    assert!(switched.contains("cargo done"), "{switched}");
    assert_eq!(sandbox.crates(), ["fd 9.0.0", "ripgrep 1.0.0"]);
    assert!(sandbox.ok(&["status"]).contains("nothing to change"));

    sandbox.write(
        "config.toml",
        "[packages]\ncargo = [\"ripgrep\", \"fd 10.0.0\"]\n",
    );
    let status = sandbox.ok(&["status"]);
    assert!(
        status.contains("~ fd 10.0.0 (upgrade from 9.0.0)"),
        "{status}"
    );
    sandbox.ok(&["switch", "--yes"]);
    assert_eq!(sandbox.crates(), ["fd 10.0.0", "ripgrep 1.0.0"]);
}

#[test]
fn an_empty_list_removes_everything_and_no_list_leaves_it_alone() {
    let sandbox = Sandbox::new("empty-list");
    sandbox.write("dotfiles/gitconfig", "");
    sandbox.write(
        "config.toml",
        &format!(
            "[links]\n{}",
            link("git", "dotfiles/gitconfig", "~/.gitconfig")
        ),
    );
    sandbox.install(&["bat 0.24.0"]);

    let status = sandbox.ok(&["status"]);
    assert!(!status.contains("bat"), "cargo is not managed:\n{status}");
    sandbox.ok(&["switch", "--yes"]);
    assert_eq!(sandbox.crates(), ["bat 0.24.0"], "left alone");

    let refused = sandbox.fails(&["cargo", "switch", "--yes"]);
    assert!(
        refused.contains("`cargo` is not declared here"),
        "{refused}"
    );
    assert!(refused.contains("`cargo = []`"), "{refused}");

    sandbox.write("config.toml", "[packages]\ncargo = []\n");
    assert!(sandbox.ok(&["status"]).contains("- bat"));
    sandbox.ok(&["switch", "--yes"]);
    assert!(sandbox.crates().is_empty());
}

#[test]
fn files_merge_and_an_empty_list_contradicts_a_listed_one() {
    let sandbox = declaring(
        "merge",
        "includes = [\"roles/dev.toml\"]\n[packages]\ncargo = [\"ripgrep\"]\n",
    );
    sandbox.write(
        "roles/dev.toml",
        "[packages]\ncargo = [\"ripgrep\", \"fd\"]\n",
    );
    let status = sandbox.ok(&["status"]);
    assert!(
        status.contains("+ fd") && status.contains("+ ripgrep") && status.contains("2 to install"),
        "{status}"
    );

    sandbox.write("roles/dev.toml", "[packages]\ncargo = []\n");
    let refused = sandbox.fails(&["status"]);
    assert!(
        refused.contains("`cargo` is declared empty here, but `config.toml` lists packages for it"),
        "{refused}"
    );
    assert!(refused.contains("roles/dev.toml:2:1"), "{refused}");
    let refused = sandbox.fails(&["switch", "--yes"]);
    assert!(refused.contains("could not be planned"), "{refused}");
    assert!(sandbox.crates().is_empty(), "nothing ran");

    sandbox.write(
        "roles/dev.toml",
        "[packages]\ncargo = [\"ripgrep 14.1.0\"]\n",
    );
    let refused = sandbox.fails(&["status"]);
    assert!(
        refused.contains("`ripgrep` is already declared as `ripgrep` in `config.toml`"),
        "{refused}"
    );
}

#[test]
fn a_host_file_applies_before_config_toml_and_can_be_named() {
    let sandbox = declaring("hosts", "[packages]\ncargo = [\"bat\"]\n");
    sandbox.write("hosts/box.toml", "includes = [\"../roles/dev.toml\"]\n");
    sandbox.write("roles/dev.toml", "[packages]\ncargo = [\"ripgrep\"]\n");

    let status = sandbox.ok(&["status"]);
    assert!(
        status.contains("+ ripgrep") && !status.contains("bat"),
        "{status}"
    );
    let status = sandbox.ok(&["--file=config.toml", "status"]);
    assert!(
        status.contains("+ bat") && !status.contains("ripgrep"),
        "{status}"
    );

    let managed = sandbox.ok(&["managed"]);
    assert!(managed.contains("* hosts/box.toml"), "{managed}");
    assert!(managed.contains("roles/dev.toml"), "{managed}");
    assert!(managed.contains("* cargo"), "{managed}");

    let unknown = sandbox.fails(&["--host=laptop", "status"]);
    assert!(
        unknown.contains("no host `laptop`") && unknown.contains("the hosts are box"),
        "{unknown}"
    );
    let outside = sandbox.fails(&["--file=../elsewhere.toml", "status"]);
    assert!(outside.contains("is outside the graph"), "{outside}");
    let both = sandbox.fails(&["--file=config.toml", "--host=box", "status"]);
    assert!(both.contains("cannot be used with"), "{both}");

    sandbox.write("hosts/box/box.toml", "");
    let ambiguous = sandbox.fails(&["status"]);
    assert!(
        ambiguous.contains("are `box`'s file; keep one"),
        "{ambiguous}"
    );
}

#[test]
fn faults_are_located_and_stop_the_run() {
    let cases: &[(&str, &[&str])] = &[
        (
            "[packages\ncargo = []\n",
            &["config.toml:1:10", "unclosed table"],
        ),
        ("[package]\ncargo = []\n", &["unknown field `package`"]),
        (
            "[packages]\ncargo = \"ripgrep\"\n",
            &["config.toml:2:9", "expected a sequence"],
        ),
        (
            "[packages]\ncargo = [\"fd 1 2\"]\n",
            &[
                "config.toml:2:16",
                "expected `<name>` or `<name> <version>`",
            ],
        ),
        ("[packages]\ncargo = [\" \"]\n", &["an empty package"]),
        (
            "[packages]\ncargo = [\"--force\"]\n",
            &["cannot start with `-`"],
        ),
        (
            "[packages]\ncargo = [\"fd\", \"fd\"]\n",
            &["declared twice to the same effect"],
        ),
        (
            "includes = [\"gone.toml\"]\n",
            &["there is no file at `gone.toml`", "config.toml:1:13"],
        ),
        (
            "includes = [\"config.toml\"]\n",
            &["`config.toml` includes itself"],
        ),
        (
            "includes = [\"../outside.toml\"]\n",
            &["`../outside.toml` is outside the graph"],
        ),
        (
            "[links]\nnvim = { source = \"nvim\" }\n",
            &["missing field `target`"],
        ),
        (
            "[links]\nnvim = { source = \"nowhere\", target = \"~/.config/nvim\" }\n",
            &["`nvim` links nothing", "there is nothing at `nowhere`"],
        ),
    ];
    for (text, expected) in cases {
        let sandbox = declaring("faults", text);
        for command in [&["status"][..], &["switch", "--yes"]] {
            let shown = sandbox.fails(command);
            for want in *expected {
                assert!(
                    shown.contains(want),
                    "{text:?} under {command:?}: no `{want}`:\n{shown}"
                );
            }
        }
    }
}

#[test]
fn a_version_is_checked_whether_or_not_its_manager_is_installed() {
    // pacman is not installed here, and holds no versions anywhere.
    let sandbox = declaring("version", "[packages]\npacman = [\"vim 9.1\"]\n");
    let shown = sandbox.fails(&["status"]);
    assert!(
        shown.contains("pacman cannot install a specific version of `vim`"),
        "{shown}"
    );
    assert!(shown.contains("config.toml:2:16"), "{shown}");
    let shown = sandbox.fails(&["switch", "--yes"]);
    assert!(shown.contains("declares what it cannot hold"), "{shown}");
}

#[test]
fn warnings_name_what_is_probably_a_mistake_and_change_nothing() {
    let sandbox = declaring("warnings", "[packages]\ncrago = [\"bat\"]\npacman = []\n");
    sandbox.write("roles/old.toml", "[packages]\ncargo = [\"fd\"]\n");
    sandbox.write("starship/starship.toml", "add_newline = false\n");
    sandbox.write("empty.toml", "");
    sandbox.write(
        "hosts/box.toml",
        "includes = [\"../config.toml\", \"../empty.toml\"]\n",
    );

    let shown = sandbox.ok(&["status"]);
    for want in [
        "`crago` names no package manager",
        "`pacman` is declared but not installed here",
        "`roles/old.toml` is included by no host file or `config.toml`",
        "`empty.toml` declares nothing",
        "nothing to change",
    ] {
        assert!(shown.contains(want), "no `{want}`:\n{shown}");
    }
    assert!(!shown.contains("starship"), "{shown}");
}

#[test]
fn a_failed_manager_fails_the_switch_but_the_links_are_placed() {
    let sandbox = Sandbox::new("failed");
    sandbox.write("dotfiles/gitconfig", "");
    sandbox.write(
        "config.toml",
        &format!(
            "[packages]\ncargo = [\"broken-crate\"]\n[links]\n{}",
            link("git", "dotfiles/gitconfig", "~/.gitconfig")
        ),
    );
    let shown = sandbox.fails(&["switch", "--yes"]);
    assert!(shown.contains("cargo failed"), "{shown}");
    assert!(
        shown.contains("could not compile `broken-crate`"),
        "{shown}"
    );
    assert!(is_link(&sandbox.home().join(".gitconfig")), "{shown}");
    let log = sandbox.home().join(".local/share/vow/logs/cargo.log");
    assert!(
        std::fs::read_to_string(log).is_ok_and(|log| log.contains("broken-crate")),
        "the failure is logged"
    );

    let status = sandbox.ok(&["status"]);
    assert!(
        status.contains("+ broken-crate") && !status.contains("gitconfig"),
        "{status}"
    );
}

#[test]
fn each_half_can_be_asked_about_alone() {
    let sandbox = Sandbox::new("halves");
    sandbox.write("dotfiles/gitconfig", "");
    sandbox.write(
        "config.toml",
        &format!(
            "[packages]\ncargo = [\"ripgrep\"]\n[links]\n{}",
            link("git", "dotfiles/gitconfig", "~/.gitconfig")
        ),
    );
    // Each half under its own heading, links under `links` alone.
    let both = sandbox.ok(&["status"]);
    assert!(
        both.starts_with("packages\ncargo\n")
            && both.contains("\n\nlinks\n  + dotfiles/gitconfig")
            && !both
                .lines()
                .any(|line| line == "config" || line == "config.toml"),
        "{both}"
    );
    let packages = sandbox.ok(&["status", "--packages"]);
    assert!(
        packages.contains("+ ripgrep") && !packages.contains("gitconfig"),
        "{packages}"
    );
    let links = sandbox.ok(&["status", "--links"]);
    assert!(
        links.starts_with("links\n  + dotfiles/gitconfig") && !links.contains("ripgrep"),
        "{links}"
    );

    sandbox.ok(&["switch", "--yes", "--links"]);
    assert!(is_link(&sandbox.home().join(".gitconfig")));
    assert!(sandbox.crates().is_empty(), "packages were out of scope");

    // Listed like the plan, with the file declaring each link.
    let listed = sandbox.ok(&["links"]);
    assert!(
        listed.starts_with("links\n    dotfiles/gitconfig  <- ~/.gitconfig  config.toml\n"),
        "{listed}"
    );
    assert!(listed.contains("every link in place"), "{listed}");
}

#[test]
fn search_and_outdated_ask_the_managers() {
    let sandbox = declaring("query", "[packages]\ncargo = [\"ripgrep 14.1.0\"]\n");
    let found = sandbox.ok(&["cargo", "search", "ripgrep"]);
    assert!(
        found.contains("cargo") && found.contains("ripgrep = \"1.0.0\""),
        "{found}"
    );
    let outdated = sandbox.ok(&["outdated"]);
    assert!(
        outdated.contains("none of those managers can report"),
        "{outdated}"
    );
}

#[test]
fn the_command_line_refuses_what_it_cannot_mean() {
    let sandbox = declaring("cli", "[packages]\ncargo = []\n");
    let cases: &[(&[&str], &str)] = &[
        (&["crago", "status"], "unknown package manager `crago`"),
        (&["cargo", "links"], "`links` takes no package manager"),
        (&["cargo", "managed"], "`managed` takes no package manager"),
        (
            &["outdated", "--links"],
            "`outdated` is about packages only",
        ),
        (
            &["search", "x", "--links"],
            "`search` is about packages only",
        ),
        (&["links", "--packages"], "`links` is about links only"),
        (&["status", "--links", "--packages"], "cannot be used with"),
        (&["--host=box", "generate-config"], "drop `--host`"),
        (
            &["pacman", "search", "x"],
            "`pacman` is not installed on this machine",
        ),
    ];
    for (args, want) in cases {
        let shown = sandbox.fails(args);
        assert!(shown.contains(want), "{args:?}: no `{want}`:\n{shown}");
    }
    let help = sandbox.ok(&[]);
    assert!(help.contains("Usage"), "{help}");
}

#[test]
fn an_upgrade_leaves_pins_alone() {
    let sandbox = declaring(
        "upgrade",
        "[packages]\ncargo = [\"ripgrep 14.1.0\", \"bat\"]\n",
    );
    sandbox.install(&["ripgrep 14.1.0", "bat 0.23.0"]);
    let dry = sandbox.ok(&["switch", "--upgrade", "--dry-run"]);
    assert!(dry.contains("$ cargo install --force bat"), "{dry}");
    assert!(
        dry.contains("1 pinned, left alone") && dry.contains("cargo: ripgrep"),
        "{dry}"
    );
    sandbox.ok(&["switch", "--upgrade", "--yes"]);
    assert_eq!(sandbox.crates(), ["bat 1.0.0", "ripgrep 14.1.0"]);
}

#[test]
fn a_switch_that_did_not_take_says_so() {
    let sandbox = declaring("did-not-take", "[packages]\ncargo = [\"ghost-crate\"]\n");
    let shown = sandbox.fails(&["switch", "--yes"]);
    assert!(
        shown.contains("applied, but 1 manager(s) did not reach the declared state")
            && shown.contains("cargo: ghost-crate not installed"),
        "{shown}"
    );

    let sandbox = declaring("unreadable", "[packages]\ncargo = [\"corrupt-db\"]\n");
    let shown = sandbox.fails(&["switch", "--yes"]);
    assert!(shown.contains("cargo: could not be read back"), "{shown}");
    assert!(sandbox.fails(&["status"]).contains("corrupt metadata"));
}

#[test]
fn naming_managers_leaves_links_alone() {
    let sandbox = Sandbox::new("managers-only");
    sandbox.write("dotfiles/gitconfig", "");
    sandbox.write(
        "config.toml",
        &format!(
            "[packages]\ncargo = [\"ripgrep\"]\n[links]\n{}",
            link("git", "dotfiles/gitconfig", "~/.gitconfig")
        ),
    );
    let dry = sandbox.ok(&["cargo", "switch", "--dry-run"]);
    assert!(
        dry.contains("+ ripgrep") && !dry.contains("gitconfig"),
        "{dry}"
    );
    sandbox.ok(&["cargo", "switch", "--yes"]);
    assert!(!is_link(&sandbox.home().join(".gitconfig")));
    let refused = sandbox.fails(&["cargo", "status", "--links"]);
    assert!(refused.contains("no package manager has"), "{refused}");
}

#[test]
fn a_hostname_matches_its_host_file_in_any_case_and_a_stray_one_is_warned_about() {
    let sandbox = declaring("hostname", "[packages]\ncargo = [\"bat\"]\n");
    sandbox.write("hosts/box.toml", "[packages]\ncargo = [\"ripgrep\"]\n");
    // macOS answers `Name.local`.
    let status = sandbox
        .vow_on(&sandbox.graph(), "Box.local", &["status"])
        .text;
    assert!(
        status.contains("+ ripgrep") && !status.contains("no host file"),
        "{status}"
    );

    let status = sandbox.vow_on(&sandbox.graph(), "laptop", &["status"]).text;
    assert!(status.contains("+ bat"), "{status}");
    assert!(
        status.contains("this machine, `laptop`, has no host file, so `config.toml` applies")
            && status.contains("the hosts are box"),
        "{status}"
    );
}

#[test]
fn an_entry_file_reached_through_a_symlink_reads_from_where_it_is() {
    let sandbox = Sandbox::new("symlinked-entry");
    sandbox.write("roles/dev.toml", "[packages]\ncargo = [\"ripgrep\"]\n");
    sandbox.write("machines/main.toml", "includes = [\"../roles/dev.toml\"]\n");
    std::os::unix::fs::symlink(
        sandbox.graph().join("machines/main.toml"),
        sandbox.graph().join("config.toml"),
    )
    .expect("symlink");
    let status = sandbox.ok(&["status"]);
    assert!(status.contains("+ ripgrep"), "{status}");
}

#[test]
fn files_applied_with_file_or_linked_by_other_machines_are_not_warned_about() {
    let sandbox = Sandbox::new("no-false-warnings");
    sandbox.write("dotfiles/a", "");
    sandbox.write("dotfiles/b", "");
    sandbox.write("dotfiles/forgotten", "");
    sandbox.write(
        "hosts/box.toml",
        &format!("[links]\n{}", link("a", "../dotfiles/a", "~/.a")),
    );
    sandbox.write(
        "hosts/laptop.toml",
        &format!("[links]\n{}", link("b", "../dotfiles/b", "~/.b")),
    );
    sandbox.write("other.toml", "includes = [\"roles/extra.toml\"]\n");
    sandbox.write("roles/extra.toml", "[packages]\ncargo = []\n");

    let status = sandbox.ok(&["status"]);
    assert!(
        status.contains("`dotfiles/forgotten` is kept here"),
        "{status}"
    );
    assert!(
        !status.contains("`dotfiles/b`"),
        "laptop links it:\n{status}"
    );

    let status = sandbox.ok(&["--file=other.toml", "status"]);
    assert!(!status.contains("is included by no host file"), "{status}");
}

#[test]
fn a_half_with_nothing_declared_says_which() {
    let sandbox = declaring("one-half", "[packages]\ncargo = []\n");
    let links = sandbox.ok(&["status", "--links"]);
    assert!(
        links.contains("no links declared in `config.toml`"),
        "{links}"
    );
    let packages = sandbox.ok(&["status", "--packages"]);
    assert!(packages.contains("nothing to change"), "{packages}");
}

#[test]
fn generate_config_writes_only_inside_the_graph() {
    let sandbox = Sandbox::new("generate-outside");
    let refused = sandbox.fails(&["cargo", "generate-config", "--file=../outside.toml"]);
    assert!(refused.contains("is outside the graph"), "{refused}");
    assert!(!sandbox.root.join("outside.toml").exists());
}

#[test]
fn a_manager_another_installs_is_converged_in_the_same_switch() {
    let sandbox = declaring(
        "staged",
        "[packages]\napk = [\"cargo\"]\ncargo = [\"ripgrep\"]\n",
    );
    sandbox.without_cargo();
    let status = sandbox.ok(&["status"]);
    assert!(
        status.contains("+ cargo") && status.contains("`cargo` is declared but not installed here"),
        "{status}"
    );

    let switched = sandbox.ok(&["switch", "--yes"]);
    assert!(
        switched.contains("now installed, so planned too: cargo"),
        "{switched}"
    );
    assert_eq!(sandbox.crates(), ["ripgrep 1.0.0"]);
    assert!(sandbox.ok(&["status"]).contains("nothing to change"));
}
