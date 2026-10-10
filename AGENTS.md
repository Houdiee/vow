# Handoff notes for agents working on vow

Read this before changing anything. It records what vow is, how the code is
laid out, and the decisions the maintainer has already made. Most of these
decisions were argued over at length, so don't reverse one without asking.

The user-facing docs are [`README.md`](README.md) (what vow is, installation,
a quick start) and [`docs/GUIDE.md`](docs/GUIDE.md) (the full reference: every
rule, with examples). Keep the guide in step with any behaviour you change.

## What vow is

A Rust CLI, `vow` ([Houdiee/vow](https://github.com/Houdiee/vow)), that keeps a machine
matching a **graph**: a folder, usually `~/.config/vow`, kept in git. Everything
is declared in TOML files in it:

```toml
includes = ["../roles/dev.toml"]     # read these too; paths relative to this file

[packages]
cargo = ["ripgrep 14.1.0", "bat"]    # a list per manager; `name` or `name version`
ghcup = []                           # declared: keep at nothing installed

[links]
nvim = { source = "../dotfiles/nvim", target = "~/.config/nvim" }
```

- **Packages half:** a list per package manager. vow drives 43 existing package
  managers (pacman, cargo, npm, brew, ...). It does not replace them.
- **Links half:** links from files kept in the graph to where they go. vow
  places symlinks that point back into the graph.

*Links* always means that half. *Config* means the whole declaration, both
halves: the TOML files, `config.toml`, `generate-config`.

`status` shows the difference between the machine and the graph. `switch` is the
only command that changes the machine. `generate-config` writes one new file;
every other command only reads. There are no commands that edit a file: with
includes, no command could know which file the user means.

## Current state

- The project is considered nearly complete, and is to be published.
- All checks pass: `cargo fmt --check`, `cargo clippy --all-targets` (no
  warnings), 259 unit tests and 41 end-to-end tests (22 in `tests/cli.rs`, 19
  in `tests/links.rs`), and the container suite for 40 managers.
- MIT licensed (`LICENSE`, copyright Houdiee). Not on crates.io yet:
  installation is `cargo install --git` or a build from source.
- Development branch: `claude/cool-hopper-p2stsj`.

## The graph

```text
~/.config/vow/
├── config.toml          # applied by a machine with no host file
├── hosts/<host>.toml    # or hosts/<host>/<host>.toml: found by hostname
└── ...                  # anything else is the user's to arrange
```

| Term | Meaning |
|-|-|
| graph | The root folder (`--path`, else `$VOW_CONFIG_DIR`, else `$XDG_CONFIG_HOME/vow` when that is absolute, else `~/.config/vow`), resolved to where it really is. One that does not exist is refused (`Graph::check`), except by `generate-config` |
| entry file | The file a machine applies: `--file` (relative to the graph, refused outside it), else `--host=NAME`'s or this machine's host file (`Graph::this_host`: the hostname as written, else its first part in any case), else `config.toml` (`Graph::entry`), always resolved to where it really is. Both host spellings at once is refused; host files existing but none this machine's is warned about |
| tree | The entry file and every file it includes, each read once, entry first (`graph::Tree`). Each `graph::File` keeps its `Source` and its parsed `Document` of `toml::Spanned` values |
| source / target / entry | A link's `source` (relative to its file, inside the graph), its `target` (absolute or `~`), and its entry: the source as a path in the graph. A link's identity is entry + target; its name is a label |
| record | `~/.local/share/vow/links`: every link vow placed on this machine, one file per machine, each line `file \t target \t entry \t source` |
| plan | The computed diff. `status`, `--dry-run` and `switch` all use the same value |

## Code map

The code flows downward through this table. Commands print what they found
with `println!`, through `report`'s helpers for nesting, colour and paths
(`report::path` writes the home directory as `~`, and every user-facing path goes
through it); `report` itself renders plans, links, diagnostics and the prompt.
`exec` prints its own output: the live boxes, or without them a line per
manager and each failed one's transcript. Nothing else prints outside tests.

| Path | What it holds |
|-|-|
| `src/cli.rs` | clap definitions only. Global flags: `--path`, `--host=NAME`, `--file=PATH` (conflicts with `--host`), `--links`, `--packages` |
| `src/main.rs` | Dispatch. `search` and `generate-config` read no declarations, so they run before `Ctx` is built and a broken file does not stop them; a manager list on `managed`/`links` is refused (`Command::takes_managers`), and a manager list narrows any command to the packages half |
| `src/commands/mod.rs` | `Ctx { graph, tree }` (`Ctx::new` refuses a tree with an unreadable file); `Ctx::files` (file names in read order); `Ctx::managers` (named, else every declared one, split into installed and not yet installed: `status`/`switch`/`outdated`; a named one must be declared, `Ctx::declared`); `installed` (named, else every installed one: `generate-config`/`search`); `Scope`; `warn` |
| `src/commands/switch.rs` | The order of a switch: plan (a manager not installed yet is validated now) → confirm → apply links → save record → `converge` (run package commands a `Stage` at a time; after each, plan, show and confirm any declared manager that is now installed) → read back what didn't take effect (`report_what_did_not_take`: an error diagnostic with a note per manager) |
| `src/commands/info.rs`, `generate.rs`, `status.rs`, `query.rs` | `managed`/`links` (the tables share `table()`: a dim header and rows indented two spaces, `*` marking what applies here); `generate_config` (writes `config.toml` or `--file` with `HEADER`, refuses an existing file); `status`; `outdated`/`search` |
| `src/graph.rs` | `Graph::{discover, root, check, within, file, name, hosts, this_host, entry, entries, sources, warnings}` (`within`: inside the graph judged by where a path really leads, through symlinks); `Document`/`LinkDecl` (serde, `deny_unknown_fields`, every value `toml::Spanned`); `File`; `Tree::{load, check, warnings, name, exists}` (includes read once, cycles/missing/outside reported at the include's span, a toml error at its own span); `resolve_inside` (a path relative to a file, refused outside the graph); `normalize`; `entries_in`; `hostname` |
| `src/packages/` | `origin.rs` (`Origin`: where a package was written; `read` parses one `Spanned` string into a spec located in its file), `spec.rs` (one package's grammar), `resolve.rs` (merge files, the repeat-in-one-list and empty-list rules, `is_managed`: a key in any file, warnings), `validate.rs` (`declared` = resolve + validate: a version only where the manager can pin, none where the name selects one, no two spellings of one name), `plan.rs` (diff against what's installed), `version.rs` |
| `src/links/` | `mod.rs` (`Links::load(tree)`: `placements` and `unlinked`; `Placement { source, entry, target, file }`, `check_overlap` (same name alike merges, else refused; one target twice refused), `check_targets`, `find_unlinked`, `checked_target`), `place.rs` (`LinkPlan`: Keep/Create/Repoint/Remove/Replaced/Blocked(`Obstacle`), and each obstacle's reason and fix), `record.rs` |
| `src/manager/` | `Manager::stage` (`Stage::{System, Toolchain, Tools}`, default `Tools`). One file per manager, or per family (`node.rs`: npm, pnpm, bun, yarn; `vscode.rs`: code, codium, code-server): command shapes, output parser, tests against real captured output. `mod.rs` holds the registry, in id order. `container.rs` is the container suite (see [Tests](#tests)) |
| `src/exec/` | Running commands. `parallel.rs`'s `Run` spans a whole switch: a box for placing links (`Run::task`) and one per manager from the start (`live.rs`, only when stdout is a terminal and colour is on), ordered by stage then name, titled `waiting` until its stage begins (`Run::stage`) and `not run` if an earlier stage failed; `pause`/`resume` around a prompt redraw what is left below it. Failure logs are written atomically to `~/.local/share/vow/logs/<manager>.log`; the error points at the one log, or at the directory when several failed |
| `src/report/` | Output helpers: `item(depth, text)`, `nested`, `marked`, `note`, `path`, `fail`, `fail_under`, `emit`, `confirm`, `plan`/`summarize`, `links` (a plan's link changes under one `links` heading), `every_link` (the `links` command: every link the same way, with its declaring file in a column), `unlinked`, style helpers |
| `src/diagnostic.rs` | `Diagnostic`/`Diagnostics`: located errors rendered with annotate-snippets; one with no place puts its notes straight under the title |
| `src/state.rs` | `~/.local/share/vow`, the only place vow keeps state |
| `src/atomic.rs` | `write` and `symlink`: temp file + rename |
| `src/testing.rs` | Unit-test helpers: `TempDir`, `seed(path, text)`, `tree(dir, files)` (writes files and loads the tree `config.toml` heads) |
| `tests/common/mod.rs` | The end-to-end sandbox: scratch home and graph, a stand-in `cargo` (a shell script keeping its installed set in a file; a crate named `broken*` fails to build, `ghost*` never appears, `corrupt*` breaks the listing) and a stand-in `apk` whose `apk add cargo` puts that cargo on `PATH` (`Sandbox::without_cargo`), `Sandbox::{vow, ok, fails, write, install, crates, record}` |
| `flake.nix` | A dev shell with the Rust toolchain (`nix develop`) for Nix users. CI and the tests don't use it |
| `.github/workflows/` | `ci.yml`: fmt, clippy `-D warnings` and the tests on every push. `managers.yml`: the container suite, one job per manager, on pushes touching `src/manager/`, weekly, and by hand |

## Decisions already made (don't reverse without asking)

1. **The user organises the graph.** vow imposes no layout: a machine applies
   one TOML file and what it includes. A host file is an ordinary file found
   by name (`hosts/<host>.toml` or `hosts/<host>/<host>.toml`); `config.toml`
   is the fallback. Anything else in the graph is the user's.
2. **The file format.**
   - Top-level keys are `includes`, `[packages]` and `[links]` only; anything
     else is an error (`deny_unknown_fields`), so a typo is caught where it is.
   - Paths (`includes`, link `source`) are relative to the file naming them
     and must stay inside the graph. Targets are absolute or start with `~`,
     with no `..`.
   - A package is `name` or `name version`; whitespace is the only separator.
   - TOML syntax and type errors are the `toml` crate's, drawn at its span;
     vow reports only what it alone can know, located through `Spanned`.
3. **No overriding, anywhere.** No file outranks another, whichever includes
   which, and there are no priorities, `mkForce`-style markers or subtraction
   syntax. The maintainer explicitly rejected silent overriding. "Everywhere
   except one machine" is a file that machine doesn't include.
4. **Packages merge Nix-style** (`packages::resolve`):
   - the same package declared alike in two files merges and installs once;
   - declared differently is an error, and that includes `ripgrep` beside
     `ripgrep 14.1.0`;
   - the same name twice in one list is an error;
   - `manager = []` in one file and a non-empty list in another is an error:
     the empty list says the manager holds nothing. Two empty lists agree.
5. **A manager with no list is not managed.** vow never removes what an
   undeclared manager has; `manager = []` keeps one at nothing installed.
   Naming an undeclared manager (`vow npm switch`) is refused.
6. **Declarations are valid or not on every machine alike.** A manager's
   lists are resolved and validated whether or not it is installed here.
7. **Links never override.**
   - Two links onto one target is an error; one name declared differently in
     two files is an error; the same name alike in two files is one link.
   - Also errors: a target inside another link's target, a target inside the
     graph, a target holding the graph, a source missing or outside the graph.
8. **Links:**
   - vow only replaces symlinks it placed itself (tracked in the record).
   - Anything else in the way (`links::Obstacle`: a file, a non-empty
     directory, a link vow didn't place) blocks the switch, except an
     **empty directory**, which is removed, because tools like `ya pkg list`
     create them.
   - Links are placed **before** package commands run, and recorded at once:
     a failed manager never undoes them. `LinkPlan::apply` runs removals
     first (so a directory link can become links to its files and back),
     re-checks each change just before making it (anything changed since the
     plan is left alone), and returns what is in place (`Applied`), which is
     always saved.
   - **The halves are independent:** a failed link stops no manager, as a
     failed manager undoes no link; each failure ends in its own error and
     the switch exits non-zero. The only ordering that stops later work is
     between stages (decision 16). The maintainer chose this explicitly.
   - **A link is its entry and its target.** Moving its declaration to
     another file or renaming it changes nothing on disk. The same pair in a
     moved graph is `Repoint` (`~`, `graph moved from <old root>`); another
     source for a target vow placed is a different link, `Replaced` (`-`)
     then `Create` (`+`), applied as one swap. The record is the previous
     applied state, diffed each switch against the current tree.
   - Whatever way a link stops applying (its entry deleted, its file deleted
     or no longer included, another host's file applied, its source deleted
     too), the next switch removes its symlink, if vow placed it and it still
     points there. `tests/links.rs` proves each case end to end.
   - Placing links is reported like a manager: a `links` box first
     (`exec::Run::task`, a row per link: `+`, `~`, `-`, or `!` with the
     reason), `links done`/`links failed` without boxes, a log at
     `logs/links.log`.
9. **Warn about anything declared but unused.** A warning is something that
   is probably a mistake but changes nothing. The maintainer wants this
   principle applied to every feature, so extend it when adding one.
   - **About this machine:** a `[packages]` key naming no manager, a manager
     declared but not installed (a switch acts on it once another manager
     installs it; the only warning narrowed by a manager list), a file this
     machine applies that declares nothing, a file beside a linked one that
     nothing links.
   - **About the graph** (`Graph::warnings`): a file that reads as vow's but
     that no entry file reaches. A tool's own TOML (`starship.toml`) does not
     read as vow's, so it is left alone.
   - The "nothing links this" warning covers only folders that hold a linked
     source, never the graph's own folder or one holding a file vow reads.
     What any machine's files link counts as linked (`Graph::sources`).
     Dotfiles count; only `.git` and vow's temporary files are skipped.
10. **Output only shows changes.** `status` and `switch` print a half only
    when it has something to change: packages a manager each, links under a
    single `links` heading. A clean machine says `nothing to change`. Every
    command takes `--packages`/`--links`; a
    command with only one half refuses the other flag. Naming managers means
    their packages only.
11. **Exit codes:** drift is an answer, so `status` exits 0. A parse error, an
    unreadable manager, a switch with no terminal to ask and no `--yes`, or a
    switch that didn't take effect (an error diagnostic: `applied, but N
    manager(s) did not reach the declared state`) exits non-zero.
12. **No templating.** Per-machine differences in a dotfile are handled by
    the tool's own include or conditional (git `[include]`/`includeIf`, ssh
    `Include`/`Match`, `source` in shells, a `hostname()` check in Lua
    configs). For a tool with neither, keep a copy per machine.
13. **Versions mean exact and reinstallable.** Managers that can't promise that
    reject versions (pacman, apt, brew, gem, ...); the guide's Versions section
    has the reasoning. Names that select a version (`node@20`) count as pinned.
14. **State lives only in `~/.local/share/vow`.** That's the record and the
    failure logs (`logs/<manager>.log`). No XDG variable moves it.
15. **Every write is atomic.** vow writes only the record, logs, links, and
    the one file `generate-config` creates; it never rewrites a file the user
    keeps.
16. **A switch runs in stages,** so one switch on a fresh machine converges.
    - System managers (apk, apt, brew, dnf, pacman, xbps, zypper), then
      toolchain/version managers (asdf, bob, elan, ghcup, mise, pyenv,
      rustup, volta, zvm), then everything else. A new manager picks its
      stage by overriding `Manager::stage`.
    - Each stage finishes before the next; managers within one run in
      parallel. A failed stage stops the rest, naming what was not run.
    - The live boxes show every manager from the start, ordered by stage
      then alphabetically, a later stage's saying `waiting` in its frame
      (and `not run` if an earlier stage failed).
    - After each stage, a declared manager that is now installed (`apt`
      listing `cargo`, `npm` listing `pnpm`) is planned, shown and confirmed
      (unless `--yes`), then runs in its stage's turn.
    - Proven end to end by the integration test
      `a_manager_another_installs_is_converged_in_the_same_switch`.
17. **A declared package is never removed.** Names are compared the way the
    manager compares them (`Manager::canonical`), and a package whose name is
    declared is never planned for removal, whatever its spelling.
18. **Removal never takes another package with it.** apt, dnf and zypper
    remove with `dpkg --remove` / `rpm --erase`, which refuse a package
    something still needs, rather than their front ends, which would also
    remove its dependents. Every removal runs without the tool's own prompt:
    vow has asked already, and runs commands with no input.

## Conventions

- **Never hand-write indentation in print strings.** Nest output with
  `report::item(depth, ...)` / `report::nested` / `report::marked`.
- **Every user-facing path goes through `report::path`,** so the home
  directory reads `~` everywhere, diagnostics included. A file of the graph is
  named by its path in it (`Tree::name`, `Graph::name`).
- **Errors in files the user wrote are diagnostics with a location.** Use
  `Origin::fault` and `Source::fault` with a `Spanned` value's span, collect
  into `Diagnostics` so a whole file is reported at once, and add a
  `.note(...)` saying how to fix it.
- **Comments are few and say why.** Tests test behaviour: a command's
  behaviour end to end in `tests/cli.rs` or `tests/links.rs`, logic that is
  hard to reach from there as a unit test using `TempDir`, `seed` and `tree`.
- **All writes go through `atomic`.** Commands are argument vectors, never
  shell strings.
- **New managers** are added only from real captured output, with a parser
  test. Managers that can't tell explicit installs from dependencies are not
  added.
- **A manager whose listing spells a name differently from how it is
  declared overrides `Manager::canonical`** (pip's `PyYAML` for `pyyaml`).
  `manager::installed` keys by it and `plan::compute` never removes a package
  whose canonical name is declared. A name the manager reads as another
  (npm's `pkg@latest`) is refused with `Manager::refuses_name`. A package the
  manager has no command to remove (volta's `node` runtime) is named by
  `Manager::keeps`: it is never planned for removal, and left with a warning.
- **Rust 2024 edition, 1.88 or newer** (`rust-version` in `Cargo.toml`);
  let-chains are used.
- Before every commit: `cargo fmt`, `cargo clippy --all-targets` (zero
  warnings), `cargo test`. Never rewrite pushed history.

## Tests

- **Unit tests** sit beside the code. Every manager's parser is tested against
  output captured from the real tool.
- **End-to-end tests** (`tests/cli.rs`, `tests/links.rs`) run the built binary
  against a scratch home and graph, with stand-in `cargo` and `apk` scripts
  (`tests/common`). They cover a whole switch, staged arrival included,
  without touching the machine.
- **The container suite** (`src/manager/container.rs`) is opt-in. Each manager
  gets its own docker or podman container. Vow's own commands install a
  package, list it and parse it, search, report what is outdated, then remove
  it and check it is gone. Every manager but `flatpak`, `code` and `codium`
  (privileged container or display) has a recipe. It skips itself unless
  `VOW_CONTAINERS` is set, and CI runs it per manager (`managers.yml`).

## Verifying a change

```sh
cargo fmt --check && cargo clippy --all-targets && cargo test
VOW_CONTAINERS=1 cargo test container          # every manager; needs docker or podman
VOW_CONTAINERS=1 cargo test container::apt     # one
```

A change under `src/manager/` also runs the container suite on push; read its
results before calling the change done.

For behaviour changes, also run the binary against a throwaway graph and home,
so the real state in `~/.local/share/vow` isn't touched:

```sh
HOME=/tmp/vh VOW_HOST=thinkpad cargo run -- --path=/tmp/g status
```

## Open items

- **Not on crates.io.** Publishing needs the maintainer's go-ahead.
- **macOS is untested.** CI runs on Linux only. Temp paths there go through
  `/private/var` symlinks, which the tests may need to resolve.
- **Listings still to make exact:** brew lists dependencies (use
  `brew leaves --installed-on-request` for removals) and never casks; mise's
  `ls --current` depends on the working directory; a failed listing reads as
  "nothing installed" for managers with `empty_until_first_install`; pip's
  `--not-required` and pacman's `-Qe` hide declared packages that are also
  dependencies; pins compare as exact strings (`5.0` vs `5.0.0`); link targets
  are compared by spelling, not by the file they reach (a target under a
  symlinked `~/.config` is not seen as the same place as its real path).
- **Optional docs idea:** a table in the guide of include/conditional idioms
  per tool, to back decision 12. It was proposed and not yet confirmed.
