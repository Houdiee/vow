# The vow guide

The full reference: every rule vow enforces, with examples, and why. For what
vow is and a quick start, see the [README](../README.md).

## The graph

The graph is a folder, usually `~/.config/vow`, kept in git. vow reads the TOML
files in it that a machine applies; everything else in it is yours, laid out
however you like:

```text
~/.config/vow/
├── config.toml              # what every machine without a host file applies
├── hosts/
│   ├── laptop.toml          # this machine, when its hostname is `laptop`
│   └── desktop/
│       └── desktop.toml     # a host may keep its own folder too
├── roles/
│   ├── dev.toml             # shared files, named and placed as you like
│   └── gaming.toml
├── dotfiles/
│   ├── nvim/
│   └── gitconfig
└── README.md
```

A file holds up to three things:

```toml
# ~/.config/vow/hosts/laptop.toml
includes = ["../roles/dev.toml"]

[packages]
cargo = ["ripgrep 14.1.0", "bat"]
pacman = ["git", "neovim", "tlp"]

[links]
nvim = { source = "../dotfiles/nvim", target = "~/.config/nvim" }
gitconfig = { source = "../dotfiles/gitconfig", target = "~/.gitconfig" }
```

- **`includes`** reads other files as if they were part of this one.
- **`[packages]`** has a list per package manager.
- **`[links]`** has an entry per link: a name, the `source` kept in the graph,
  and the `target` where its symlink goes.

Every path a file writes -- an include, a link's source -- is relative to that
file, and stays inside the graph: a graph cloned from somewhere should not be
able to reach outside itself. Where a path leads is what counts, so a symlink
kept in the graph cannot take it outside either. Anything else at the top of a file is an error,
so a typo such as `[package]` is caught where you made it.

### Which file a machine applies

1. `--file=PATH`, a path in the graph, if given;
2. else its host file, `hosts/<hostname>.toml` or
   `hosts/<hostname>/<hostname>.toml`, if it has one;
3. else `config.toml`.

A hostname is matched as written, else by its first part in any case, since
macOS answers `Name.local` and some machines a name with their domain. When the
graph has host files but none is this machine's, `status` and `switch` warn that
`config.toml` applies instead, naming the hosts there are; `VOW_HOST` names the
machine when its hostname will not do.

`--host=NAME` applies another machine's host file -- `switch --host=desktop`
converges this machine on what *desktop* gets, which is how one machine takes
over another's role -- and is refused if that host has no file. A host with
both `hosts/<name>.toml` and `hosts/<name>/<name>.toml` is refused too, since
either could be the one meant, and so is a `--file` outside the graph. A machine with
no host file applies `config.toml`, which is what lets a freshly cloned graph
converge a new machine with no setup; a graph with no `config.toml` declares
nothing until you write one.

Nothing else is special. A host file is an ordinary file that happens to be
found by name; it includes whatever you want to share, and how you split things
up -- roles, a common base, one file per tool -- is up to you:

| machines | one way to lay it out |
|-|-|
| one | `config.toml` |
| two or three, each a little different | `config.toml` holding what they share, and a host file each that includes it |
| several sharing roles (dev boxes, servers, a gaming PC) | a file per role, and host files including the roles they take |

Every command takes `--path`, so the graph can live anywhere — beside a dotfiles
repo, on a removable disk, or several side by side. It takes precedence over
`VOW_CONFIG_DIR`, so one run can read another graph without disturbing a shell
that has it set. A graph that is not there is refused rather than read as
declaring nothing: a typo in `--path`, or a disk not mounted, would otherwise
have a switch remove every link it placed. `generate-config` is the one command
that starts a graph.

### Includes

An include is read once, however many files name it, so two roles can both
include a common base. A file that ends up including itself is an error, and
so is an include that is missing or outside the graph:

```console
$ vow status
error: there is no file at `roles/devv.toml`
 --> ~/.config/vow/hosts/laptop.toml:1:13
  |
1 | includes = ["../roles/devv.toml"]
  |             ^^^^^^^^^^^^^^^^^^^^^ included here
  |
  = note: a path in `includes` is relative to the file naming it
```

**Nothing overrides anything.** Every file a machine applies counts the same,
whichever includes which, and what they declare is merged:

- what two files declare alike is one declaration;
- what they declare differently is an error naming both, never settled by which
  file came first.

"Everywhere except one machine" is a file that machine does not include; there
is no way to subtract, on purpose.

## Packages

`[packages]` has a list per package manager. A package is its name, or its name
and a version, separated by a space:

```toml
[packages]
brew = ["node@20"]             # a formula NAME -- brew ships versioned formulae
cargo = ["ripgrep 14.1.0"]     # held at that version
pacman = ["vim", "ttf-fira-code"]
ghcup = []                     # keep ghcup at nothing installed
```

**A manager with a list is managed; one without is left alone.** For each
manager listed, `switch` installs what the list names and removes what it does
not. A manager no file lists is not managed at all -- vow never removes what it
has -- so `ghcup = []` is how to keep one at nothing installed. A key naming no
manager is warned about.

**Whitespace is the only separator.** That is deliberate: it is the one
character no package manager permits inside a package name. Every other
candidate is load-bearing somewhere — `@` in Homebrew formula names and npm
scopes, `=` in apt's own pin syntax, `:` in OCI digests. Names are therefore
opaque, and `node@20` means the formula called `node@20`.

A package listed twice in one list is an error, whatever the manager. A list
declares each package once. So is a name starting with `-`: `pacman -S
--noconfirm` is a very different command from installing a package called
`--noconfirm`. Commands are built as argument vectors, which stops a name
changing the *shape* of a command; it does not stop a name being read as an
option.

### A union across files

A single list of packages cannot describe two machines. Your laptop needs
`tlp`; your desktop needs `nvidia`. Sync one list between them and convergence
will happily uninstall whichever one it doesn't know about. So each machine's
list is the union of the files it applies, merged the way Nix merges modules:

```toml
# config.toml -- shared
[packages]
pacman = ["vim", "ripgrep"]

# hosts/laptop.toml
includes = ["../config.toml"]
[packages]
pacman = ["tlp"]
```

A package declared alike in two files is installed once. Declared differently
-- including `ripgrep` beside `ripgrep 14.1.0`, since one asks for whatever is
current and the other for one version -- it is an error. So is an empty list in
one file beside a list of packages for the same manager in another: `pnpm = []`
says pnpm holds nothing, so no other file can add to it. Two empty lists agree,
and an empty list merges with any file that does not list that manager at all.
Make them agree, or declare it in one file:

```console
$ vow status
error: `ripgrep` is already declared as `ripgrep 14.0.0` in `hosts/laptop.toml`
 --> ~/.config/vow/config.toml:3:21
  |
3 | pacman = ["vim", "ripgrep 14.2.0"]
  |                  ^^^^^^^^^^^^^^^^ declared differently here
  |
  = note: first declared at ~/.config/vow/hosts/laptop.toml:4; files merge only what they declare alike, and none overrides another: make them agree, or declare the package in one file
error: 1 manager(s) could not be read; nothing above includes them
```

**A name means what the manager means by it.** Where a manager lists a package
under another spelling, vow compares names the way that manager does, so a
declaration is never installed under one name and removed under the other:

| manager | the same package |
|-|-|
| pip, pipx, uv | `pyyaml`, `PyYAML`; `typing-extensions`, `typing_extensions`; `requests[socks]`, `requests` |
| code, codium, code-server | `github.copilot`, `GitHub.copilot` |
| brew | `user/tap/pkg`, `pkg` |
| flatpak | `org.gnome.Calculator//stable`, `org.gnome.Calculator` |
| rustup | `stable-x86_64-unknown-linux-gnu`, `stable` on that machine |

Two spellings of one package are an error (`` `PyYAML` and `pyyaml` are the
same package to pip ``), and so is a name the manager would read as something
else: npm's `pkg@latest` installs `pkg`, so the version goes after a space
instead.

## Diagnostics

Every file is one you write by hand, so anything wrong with one is reported
where you wrote it — file, line, column, and an arrow under the part that is
wrong. TOML itself -- a missing bracket, a string where a list belongs -- is
read by the [toml](https://crates.io/crates/toml) crate, and its message is
drawn at the place it names:

```console
$ vow status
error: unclosed table, expected `]`
 --> ~/.config/vow/config.toml:1:10
  |
1 | [packages
  |          ^ here
```

What only vow can know -- a version a manager cannot hold, a conflict between
files, an include that is not there -- points at the value it is about:

```console
$ vow flatpak status
error: flatpak cannot install a specific version of `org.gnome.Loupe`
 --> ~/.config/vow/config.toml:4:30
  |
4 | flatpak = ["org.gnome.Loupe 47.0"]
  |                             ^^^^ drop the version
  |
  = note: only a manager that can reinstall an exact version later accepts one
error: 1 manager(s) could not be read; nothing above includes them
```

A manager whose declarations do not check out cannot be compared with the
machine, so `status` ends by saying how many it left out, and exits non-zero.
Declarations are checked whether or not their manager is installed here, so a
file is valid or not the same way on every machine: `pacman = ["vim 9.1"]` is
an error on a Mac too. A
file that cannot be read at all stops everything, since nothing it declares can
be trusted to be complete.

Where a fault is about two places in one file, both are shown — the one to
change carries the arrow and the one it contradicts is drawn beside it. Across
two files, the note names the other place.

Three things follow from treating these as diagnostics rather than messages:

- **A file is reported whole.** Every bad value in it comes back at once,
  because fixing one to be shown the next is worse than being shown all of
  them.
- **Warnings look the same as errors** and carry the same places. A warning is
  not a lesser kind of message, only a lesser kind of problem.
- **A fault with no line says so.** Where something has no place to point at —
  a file in a link's way — it names the file and draws no arrow, rather than
  pointing at line 1 and being wrong.

### Warnings

An error stops vow; a warning is something that is probably a mistake but
changes nothing about what vow would do. `status` and `switch` say them first,
in one block; naming managers narrows only the not-installed warning, to those
managers:

| warning | why it is worth saying |
|-|-|
| a key in `[packages]` naming no manager, such as `crago` | its packages are silently ignored |
| a manager declared but not installed here | a switch acts on it once another manager installs it, and skips it otherwise |
| a file or directory beside a linked one that no link names | it is kept, but never placed |
| a file this machine applies that declares nothing | fill it or delete it |
| a file of declarations no machine reaches -- included by no host file or `config.toml` | nothing it declares reaches any machine |

The last is about the graph rather than this machine, so every machine says it.
Only a file that reads as vow's counts, so a tool's own TOML kept in the graph
-- `starship.toml`, say -- is left alone.

```console
$ vow status
warning: `dotfiles/tmux` is kept here, but nothing links it
 --> ~/.config/vow/dotfiles/tmux
  = note: give it a link, or move it out of this folder
```

Only folders that hold a linked source are checked, and everything in them
counts, dotfiles included -- only `.git` and vow's own temporary files are
passed over. The graph's own folder, and any holding a file vow reads, also
hold the rest of the graph, so nothing there is expected to be linked.

Rendering is [annotate-snippets](https://crates.io/crates/annotate-snippets),
the crate behind rustc's own diagnostics, so columns, tabs, long lines and
folding behave the way they do in a compiler error.

## Links

`[links]` says where the files kept in the graph belong:

```toml
[links]
nvim = { source = "dotfiles/nvim", target = "~/.config/nvim" }
gitconfig = { source = "dotfiles/gitconfig", target = "~/.gitconfig" }
backup = { source = "bin/backup", target = "~/.local/bin/backup" }
```

The `source` is a file or directory in the graph, relative to the file
declaring the link. The `target` is where the symlink goes: an absolute path,
or one starting with `~`; a trailing `/` names the same place. The name is
yours, for reading and for errors.

**The content stays in the graph and the link points at it**, never the other way
round. That is what keeps the graph the single copy of your dotfiles: it can be
versioned, moved or cloned onto a new machine without anything being duplicated,
and editing `~/.config/nvim/init.lua` edits the file in the graph.

A link is its source and its target, wherever it is declared and whatever it is
called: moving its line to another file, or renaming it, changes nothing on the
machine. A different source or target is a different link.

However a link stops applying to this machine, the next `switch` removes its
symlink: deleting its entry, deleting the file that declared it, dropping the
include that reached that file, applying another host's file, or deleting the
source from the graph along with its entry. `status` shows each as a `-` row
first. Only a symlink vow placed and that still points where vow left it is
removed; a link someone has repointed, or a file now standing in its place, is
left alone.

A source that is not there is an **error**: it would leave a switch half done.
So are links that cannot be placed safely, whichever files declare them:

- **two links onto one target** -- order would decide the winner;
- **one name declared differently in two files** -- the same name alike in two
  files is one link;
- **a target inside another link's target** (`~/.config/nvim` and
  `~/.config/nvim/init.lua`) -- the outer link points into the graph, so the
  inner one would be written there;
- **a target inside the graph** -- the graph would link to itself -- or **one
  holding the graph**, which a link there would replace;
- **a source outside the graph, or the graph itself**, and a target that goes
  back up with `..`.

```console
$ vow status
error: `~/.bashrc` is already linked by `bashrc` in `config.toml`
 --> ~/.config/vow/hosts/laptop.toml:5:52
  |
5 | bashrc-laptop = { source = "bashrc", target = "~/.bashrc" }
  |                                               ^^^^^^^^^^^ linked again here
  |
  = note: first declared at ~/.config/vow/config.toml:3; files never override each other: declare the link once, in a file every machine that wants it includes
```

So a file that differs between machines is linked from a file each of those
machines includes, not from one they all share.

**Per-machine values belong in a second file, not a template.** Most tools can
include another file -- git's `[include] path = ~/.gitconfig.local`, ssh's
`Include ~/.ssh/config.local`, a shell's `source ~/.zshrc.local` -- so link the
shared file from a shared TOML file and the small machine-specific one from the
host file. Both stay real files you edit in place.

### Placing links

`status` compares each link with what is actually on disk, and `switch` makes
them match. The changes are listed under `links`, with the same markers the
package half uses:

```console
$ vow status
links
  + dotfiles/gitconfig  <- ~/.gitconfig
  - dotfiles/tmux.conf  <- ~/.tmux.conf
1 to link, 1 to unlink
```

| mark | meaning |
|-|-|
| `+` | nothing there yet; the link will be created |
| `~` | the same link in a graph that has moved; it is repointed, and the row says from where: `(graph moved from ~/old/vow)` |
| `-` | a link vow placed is no longer declared, or another source now takes its target; it is removed |
| `!` | something vow did not place is in the way |

A new source for a target vow placed is shown as `-` and `+`, and swapped in a
single rename so the target is never missing.

**vow only ever changes a link it placed itself.** It keeps one record of those
per machine, `~/.local/share/vow/links`, outside the graph since the graph may
be shared. The record is not every link the graph declares: it is what the last
switch left on this machine. Each switch compares it with what the files this
machine applies declare now, so a link appears when a file declaring it starts
applying and goes when it stops, as when a host file includes another role or
you switch as another host. Nothing else could say a link is gone, once no file
names it. A link stays vow's while it still points where the record says vow
pointed it, whatever has happened since:

- **the graph moved** -- the old links still match the record, so the next
  switch repoints them to the new place, and removes those no longer declared;
- **the machine was renamed** -- nothing changes, unless the new name has a host
  file of its own;
- **you switch to another graph**, with `--path` or as another host -- the
  machine converges to that graph, which includes removing the first graph's
  links that it does not declare.

A real file, a directory with anything in it, or a symlink vow did not make is
never replaced: it blocks the link, and `switch` refuses before changing
anything:

```console
$ vow switch
error: cannot link `~/.tmux.conf`: a file is in the way
 --> ~/.tmux.conf
  = note: move it into the graph and link it from there, or delete it
```

A directory with something in it gets the same advice. A symlink vow did not
place says `a link vow did not place is in the way`, with the note `remove that
link; vow only changes links it placed`.

The one thing in the way vow will clear is an **empty directory**: it holds
nothing to lose, and tools make them just by running -- `ya pkg list` creates
`~/.config/yazi` when it is missing, so a `status` would otherwise leave a
directory blocking the link it was reporting. vow removes it deepest first with
the OS's own "only if empty" removal, so a file that appears meanwhile stops it
rather than being lost.

Links are placed **before** packages are installed, so a manager that writes
into a linked folder -- `ya pkg add` writes `~/.config/yazi/package.toml` --
writes through the link into the graph, rather than making a directory in the
link's way.

Each link is swapped into place with a single rename, so an interrupted switch
leaves every link either as it was or as declared, never missing.

Removals go first, so a directory link can become links to the files in it, or
the reverse, in one switch: a link from `dotfiles/nvim` to `~/.config/nvim`
replaced by one from `dotfiles/nvim/init.lua` to `~/.config/nvim/init.lua`
removes the directory link, then links the file into a real `~/.config/nvim`.
Joining them back works the same way, since a directory holding only vow's
links that are going is no obstacle.

A switch checks each link again just before changing it. Anything that appeared
or changed while you read the plan -- a file written where a link was to go,
one of vow's links pointed elsewhere -- is left alone and reported as `!` with
`changed since the plan; left alone`.

`vow links` lists every link that applies, whether or not it would change, the
way `status` does, with the file declaring each one beside it. Then come the
entries nothing links, which `status` and `switch` warn about. Each row names
the source by its path in the graph:

```console
$ vow links
links
    dotfiles/nvim       <- ~/.config/nvim  hosts/laptop.toml
  + dotfiles/gitconfig  <- ~/.gitconfig    hosts/laptop.toml
    shell/zshrc         <- ~/.zshrc        roles/dev.toml
1 to link

not linked
  dotfiles/notes
```

Every command takes `--links` and `--packages` to speak about one half only.
A command that only has one half -- `outdated`, `search` and `generate-config`
are packages only, `links` is links only -- refuses the other flag rather than
printing nothing.

## Safety

Converging in both directions means `vow` can uninstall things. The design
assumes that is dangerous:

- **Non-zero means vow failed, not that it found something.** A machine that
  has drifted is an answer, so `status` exits zero and says so; a graph that
  will not parse, or a switch that ran and did not take, is a failure and
  exits non-zero.
- **Only `switch` changes a machine.** Every other command reads the machine or
  the graph; `generate-config` writes one new file and nothing else.
- **`switch` shows a plan and asks.** Answering no stops the whole switch,
  links included. With no terminal to ask, consent is never inferred:
  nothing changes, and the switch fails saying to pass `--yes`, so a script
  never reads it as success.
- **`--yes` is the whole opt-out.** It skips the prompt for installs and
  removals alike, so a script that passes it has said yes to both.
- **`--dry-run` cannot lie.** `status`, the dry run, and the real switch all
  compute and execute the same `Plan` value, so the preview is the action. A
  dry run changes nothing at all, vow's own record included.
- **Declaring a package is what keeps it.** Removal only ever targets packages
  nothing declares, for managers that are declared at all, and
  `vow generate-config` records everything installed — kernel included. vow
  keeps no list of packages it secretly refuses to touch.
- **Removal never touches configuration.** `dpkg --remove`, not `--purge`;
  `pacman -Rs`, not `-Rns`. Deleting a package's configuration is not a
  decision a package-list sync gets to make.
- **Removal never takes another package with it.** `apt-get remove`, `dnf
  remove` and `zypper remove` also remove whatever depends on a package, which
  may be declared, so vow removes with `dpkg --remove` and `rpm --erase`
  instead. Like `pacman -Rs` and `xbps-remove`, they refuse a package something
  still needs, and that manager's step fails saying what needs it. Removals run
  without a prompt of their own: vow has already asked.
- **Links never replace what vow did not place.** A file, a directory with
  anything in it, or a foreign link in a link's way blocks the switch; only
  vow's own links are repointed or removed, and only an empty directory is
  cleared. Whatever is in place after a switch is recorded, even when a link
  or a manager failed, so vow never mistakes its own links for someone
  else's.
- **Every write is atomic.** The file `generate-config` writes, the record and
  each link are built beside their destination and renamed over it.

## Versions

A version means one thing everywhere: **this exact version, installable again at
any time**. A manager that cannot make that promise does not accept versions at
all — declaring one is an error, never an approximation.

| manager | versions | why |
|-|-|-|
| cargo | **yes** | crates.io is immutable; yanked crates still install by exact version |
| npm, pnpm, bun, yarn | **yes** | the registry is immutable; unpublish is restricted |
| pip, pipx, uv | **yes** | PyPI keeps published releases |
| pixi | **yes** | conda-forge keeps published builds |
| dotnet | **yes** | NuGet is immutable |
| composer | **yes** | Packagist keeps published releases |
| go | **yes** | the module proxy is immutable |
| pub, opam, coursier | **yes** | immutable package repositories |
| code, codium, code-server | **yes** | the marketplace keeps published extension versions |
| mise, asdf, ghcup, volta | **yes** | a version manager's whole job is exact versions |
| apt, dnf, zypper | **no** | old versions are pruned from the archives |
| apk, xbps | **no** | one version per release branch |
| brew | **no** | no general way to install an old version — use `node@20` |
| flatpak | **no** | builds are addressed by commit, not version |
| luarocks | **no** | takes a version as a separate argument, not per package |
| fisher, krew, raco | **no** | they report no version, so a pin could never be checked |
| soar | **no** | installing an old version back is unverified |
| yazi | **no** | a plugin tracks a branch; `ya pkg add` takes no revision |
| **gem** | **no** | versions coexist, so a pin can never converge (below) |
| **pacman** | **no** | excluding a pin from an upgrade is a partial upgrade (below) |

Some managers select the version in the name instead — `node@20` for brew,
`llama3:8b` for ollama, and the whole name for bob, elan, pyenv, rustup and zvm
(`0.10.0`, `3.12.8`, `1.75.0`). Those count as pinned, so `vow switch --upgrade`
leaves them where they are. A name that already selects a version *and* a
version is a contradiction: `node@20 20.11.0` is rejected.

### When a version can be pinned at all

Two things have to be true, and they rule out more managers than you would guess.

**The manager must keep exactly one version of a package.** RubyGems does not:

```console
$ gem install tilt -v 2.0.11 && gem install tilt -v 2.3.0
$ gem list | grep tilt
tilt (2.3.0, 2.0.11)        # both, side by side
```

Pinning `tilt 2.0.11` there would have vow read the newest (2.3.0), install
2.0.11 — which succeeds and removes nothing — and report the same drift on every
run afterwards, forever.

**A package must own its own dependencies.** This is what rules out pacman. If
`A` is pinned and `B` depends on a newer `A`, then upgrading `B` while holding
`A` is a *partial upgrade*, which Arch does not support and which leaves `B`
linked against a version of `A` that is not installed. `pacman --ignore` will do
it and only warn.

### What vow does when a version will not move anyway

It cannot force a manager to honour a pin. What it can do is refuse to claim
success. After a switch runs, vow reads the installed state back and names
anything the run was supposed to change but did not -- here through a pipe, so
without the live boxes:

```console
$ vow cargo switch --yes
cargo
  ~ ripgrep 14.1.0 (upgrade from 14.0.0)

1 to upgrade
cargo done
error: applied, but 1 manager(s) did not reach the declared state
  = note: cargo: ripgrep still at 14.0.0
  = note: check each declared name and version exists
```

A package that was to be installed and is not says `not installed`; one that
was to go and is still there says `still installed`, with the note `check
nothing still installed depends on what was to go`.

A command that exits zero and changes nothing is the one failure a convergence
tool must not report as success. This catches the whole class: a pin fighting a
dependency, a manager that declines to downgrade, a repin that quietly no-ops.


## Commands

```text
vow [MANAGERS] <command>
```

Managers come first and are comma-separated.

| Command | Action |
|-|-|
| `vow [managers] status` | show how this machine differs from what it declares |
| `vow [managers] switch` | converge (plan, confirm, execute) |
| `vow [managers] switch --upgrade` | also bring unpinned packages up to date |
| `vow [managers] switch --dry-run` | show the plan and stop |
| `vow [managers] generate-config` | write `config.toml` (or `--file`) declaring everything installed; never overwrites |
| `vow [managers] generate-config --pin` | the same, holding each package at its installed version where its manager can |
| `vow [managers] outdated` | declared packages with a newer version available |
| `vow [managers] search <query>` | search each manager's own index |
| `vow links` | every link that applies, which file declared it, and whether it is in place |
| `vow managed` | every manager vow knows and what this machine does with each, then each file read |

```console
$ vow status                        # every managed manager
$ vow cargo,npm status              # just these two
$ vow pacman,brew generate-config   # declare only these two
```

When none is named, `status`, `switch` and `outdated` act on every managed
manager -- installed, with a list in a file this machine applies -- and
`generate-config` and `search` on every installed one. A manager named for
`status`, `switch` or `outdated` must have a list here: with none, nothing says
what it should hold, and `vow npm switch` would otherwise remove everything npm
has. `npm = []` is how to keep it at nothing installed. Naming managers is about
their packages, so links are left alone, and `--links` beside a manager list
is refused. `managed` and `links` take no manager list:

```console
$ vow cargo links
error: `links` takes no package manager; drop `cargo`
```

Every command takes `--path`, `--links` and `--packages`, and the commands
that read declarations take `--host=NAME` and `--file=PATH` to apply another
file than this machine's (see [Which file a machine
applies](#which-file-a-machine-applies)).

There is no command to edit a file. With includes, no command could know which
file you mean, so declarations are edited the way they are read: by hand.

### outdated

Read-only. A newer version existing is not a fault: an entry without a version
means "any version", so the declaration is already satisfied. The two cases are
separated because only one is vow's to act on:

```console
$ vow npm outdated
npm
  typescript 5.4.5 -> 5.6.3
  prettier -> 3.3.3 (unpinned; an ordinary upgrade picks this up)

1 pinned package(s) behind.
```

A manager that could not be asked at all is reported, and `outdated` exits
non-zero, rather than counting it as up to date. A declared manager that is not
installed here is named as not asked.

### switch --upgrade

The one upgrade only vow can run, because no package manager knows which of its
packages you pinned. Most managers have no flag to exclude a package but upgrade
named ones happily, so vow hands them the unpinned ones:

```console
$ vow cargo switch --upgrade --dry-run
cargo
  $ cargo install --force bat fd-find

note: 1 pinned, left alone:
  cargo: ripgrep
dry run: nothing changed
```

Arch is the exception. It forbids partial upgrades, and pacman takes no versions
(see [Versions](#versions)), so its upgrade is always the whole system:

```console
$ vow pacman switch --upgrade --dry-run
pacman
  $ sudo pacman -Syu --noconfirm
dry run: nothing changed
```

It is a flag rather than a command because exactly one command changes a
machine. The commands are printed before the prompt: an upgrade is not
expressible as a package diff, so the commands themselves are the plan.

### search

A pass-through: it runs each manager's own search and shows the output
unchanged. Nothing is parsed, because a search result is prose — descriptions,
relevance ordering, highlighting — and vow has no business reshaping it. What it
adds is the question only vow can answer: of the managers *you* use, which have
this?

```console
$ vow search ripgrep
cargo
  ripgrep = "15.2.0"    # recursively searches directories for a regex pattern

pacman
  extra/ripgrep 15.2.0-1
      A search tool that combines the usability of ag with the raw speed of grep

```

Like `generate-config`, and unlike the other commands, `search` considers every
manager installed on the machine rather than only managed ones — deciding where
to install something from is the point.

Bare `vow` prints help rather than doing anything.

### generate-config

Writes one new file declaring what each installed manager has, or only the
managers named: `config.toml` at the top of the graph, or the path `--file` gives.
A file that exists is never rewritten -- folding today's installed set into a
declaration would undo what `switch` prunes -- so it refuses instead. The file
starts with comments explaining the format, then `includes = []`, a list per
manager, and an empty `[links]`.

It records **names only** unless you pass `--pin`, so ordinary upgrades stay
none of vow's business — a declared package without a version means "any
version", and `pacman -Syu` upgrading it is not drift. With `--pin`, each
package is held at the version installed now, for the managers that can hold
one. If this machine applies a host file that does not include what was
written, it says so.

To pin or unpin later, edit the file: `"ripgrep 14.1.0"` holds a version,
`"ripgrep"` tracks whatever is current. `vow managed` lists every file this
machine reads.

## How a switch runs

Managers run in three stages, each finishing before the next starts, because
each can provide what a later one needs:

1. **system** — apk, apt, brew, dnf, pacman, xbps, zypper: the libraries a build
   links against, and often a toolchain itself (`apt` can install `cargo`);
2. **toolchains** — asdf, bob, elan, ghcup, mise, pyenv, rustup, volta, zvm:
   `cargo`, `npm`, `python`;
3. **everything else** — the language, plugin and editor managers those provide.

So `libssl-dev` in the `apt` list is in place before `cargo` compiles against it,
and a manager missing when the switch began can arrive during it. After each
stage, a declared manager that is now installed is planned and shown, and asked
about unless `--yes`, since nobody has seen its plan yet:

```console
$ vow switch --yes
warning: `pnpm` is declared but not installed here
 --> ~/.config/vow/config.toml:3:1
  |
3 | pnpm = ["is-odd"]
  | ^^^^ not installed
  |
  = note: a switch acts on it once another manager installs it -- list it as a package of one, as `apt` can list `cargo` -- and skips it otherwise

npm
  + pnpm

1 to install
npm done

note: now installed, so planned too: pnpm
pnpm
  + is-odd

1 to install
pnpm done
```

One `vow switch` on a fresh machine therefore reaches the declared state, rather
than needing a second run for whatever the first one installed. A stage that
fails stops the switch before the next, which may rely on it, and vow names the
managers it did not run.

Every step of the switch gets a box from the start -- `links` first, then
each manager ordered by stage and then by name -- so the screen reads in the
order things happen. A manager whose
stage has not begun says `waiting` in its frame:

```text
╭─ rustup ───────────────────────────────────────────────────────────────╮
│ $ rustup toolchain install stable                                      │
│ info: downloading component 'rustc'                                    │
╰────────────────────────────────────────────────────────────────────────╯
╭─ cargo waiting ────────────────────────────────────────────────────────╮
│                                                                        │
╰────────────────────────────────────────────────────────────────────────╯
╭─ npm waiting ──────────────────────────────────────────────────────────╮
│                                                                        │
╰────────────────────────────────────────────────────────────────────────╯
```

If a stage fails, the boxes after it say `not run`. When a switch stops to ask
about a manager that has just arrived, the boxes so far stay where they are,
and the ones still to run are drawn again below the question.

Within a stage, every manager runs in its own thread, because they share
nothing — `npm` has no reason to wait on `cargo`. Each box is redrawn in place,
so two managers running at once never write over each other. A box holds the last five
lines that manager printed (fewer on a short terminal), and keeps them with `done` in its header once it has
finished, while the slower ones carry on:

```text
╭─ cargo done ───────────────────────────────────────────────────────────╮
│ $ cargo install ripgrep                                                │
│    Compiling ripgrep v14.1.1                                           │
│   Installed package `ripgrep v14.1.1` (executable `rg`)                │
╰────────────────────────────────────────────────────────────────────────╯
╭─ npm failed ───────────────────────────────────────────────────────────╮
│ $ npm install -g typescript                                            │
│ error: could not resolve libfoo >= 2.0                                 │
│ `npm install -g typescript` failed: exited with status 1               │
╰────────────────────────────────────────────────────────────────────────╯
```

A carriage return counts as a line ending, so a progress bar redrawing itself
arrives as successive lines instead of one enormous one, and every escape a
manager emits is stripped — cursor movement from inside a box would otherwise
draw over the boxes below it. Rows are measured in terminal **columns**, not
characters: `安` is one character and two columns, and a row padded by character
count is wider than it was padded for, wraps, and drags every box below it out
of place. Box-drawing characters are used where the locale says they will
render and `+-|` where it does not, since mojibake would be worse than plain
corners.

**A failure does not take the view down.** The manager that failed says so in
its header and keeps the reason in its box, while the others carry on. One
manager failing two seconds in should not leave you watching `failed` for the
four minutes the slow one takes, and should not rip the view away from the
managers still working either. Nothing in the stage is aborted: they share
nothing, so stopping a half-finished `cargo` would be worse than letting it
finish. The stages after it are not run.

When the boxes will not fit, the depth shrinks to suit; below one line each vow
gives up on them and holds every manager's output until the run ends. Same for a
terminal it cannot measure, for output that is piped or redirected, and when
`NO_COLOR` is set.

The boxes stay where they are when the run ends. Each one has already said how
its manager finished — `done` or `failed` in its header, and for a failure the
last lines it printed, which is where the complaint is. There is no second
telling underneath: an error is just an error, and the box already showed it.

```text
╭─ npm done ─────────────────────────────────────────────────────────────╮
│ $ npm install -g typescript                                            │
│ added 1 package in 2s                                                  │
╰────────────────────────────────────────────────────────────────────────╯
╭─ cargo failed ─────────────────────────────────────────────────────────╮
│    Compiling sccache v0.8.1                                            │
│ /usr/bin/ld: cannot find -lssl                                         │
│ `cargo install sccache` failed: error: could not compile `sccache`     │
╰────────────────────────────────────────────────────────────────────────╯
error: 1 of 2 manager(s) failed: cargo
 --> ~/.local/share/vow/logs/cargo.log
```

Three hundred lines of `Compiling foo v1.2.3` have no business on your screen,
so the whole of a failed manager's output goes to a file as well —
`~/.local/share/vow/logs/<manager>.log`, one per manager, written whole and
overwritten each run, so nothing grows and there is nothing to clean up. The
closing error says how many failed and points at the log when one did, or at
the directory of logs when several did.

Where there were no boxes — piped, redirected, or a terminal too small — each
manager gets one line once they have all finished: `cargo done` for one that
succeeded, and `npm failed` for one that did, followed by its whole transcript,
because then nothing else has shown it to you.

If a stage needs root, vow asks once before it starts — workers capture their
output, so an elevator prompting inside one would block with nothing on screen
to explain why.

Links are placed first, so a package that writes into a linked folder on
install writes into the graph. They are recorded straight away and stay placed whatever a
manager does afterwards: a failed `npm` is no reason to leave your dotfiles
unplaced, and the next switch retries only what failed. The reverse holds too:
a link that cannot be placed stops neither the other links nor any manager.
Everything that did go in is recorded, and the switch ends with an error for
each half that failed. The two halves are independent; the one ordering vow
keeps is between stages, since a later stage can genuinely need an earlier
one.

Placing links is reported the way a manager is: a `links` box ahead of every
manager's, with a row per link, or without boxes a `links done` or
`links failed` line, a failure's transcript, and its log:

```console
links failed
  ! dotfiles/git <- ~/.config/git: could not link ~/.config/.git.vow-tmp: File exists (os error 17)
  + dotfiles/nvim <- ~/.config/nvim
  could not place 1 of 2 link(s)
cargo done
error: could not place 1 of 2 link(s)
 --> ~/.local/share/vow/logs/links.log
```

Installs run before removals, so a package moving between
managers is never briefly absent and an interrupted run leaves more installed
rather than less.

## Supported managers

```text
apk   apt    asdf    bob     brew      bun   cargo  code   code-server
codium   composer  coursier  dnf    dotnet  elan   fisher  flatpak
gem   ghcup  go      krew    luarocks  mise  npm    ollama  opam
pacman   pip    pipx    pixi   pnpm      pub   pyenv  raco    rustup
soar  uv     volta   xbps    yarn      yazi  zvm    zypper
```

Distro managers, language managers, version managers, plugin managers and
editor extensions alike — a declaration is a declaration. A manager is acted on
when its executable is on `$PATH` *and* a file this machine applies has a list
for it, which are the two columns of `vow managed`; `*` marks the managers that
are both:

```console
$ vow managed
  manager      installed  declared
  apk          -          -
...
* cargo        yes        yes
...
  npm          yes        -
...

  file               packages    links
* hosts/laptop.toml  pacman      1
  roles/dev.toml     cargo, npm  3
  config.toml        pacman      -

applies: ~/.config/vow/hosts/laptop.toml
graph: ~/.config/vow
```

The second table lists every file this machine reads, in the order read, with
the managers each lists and how many links it declares; `*` marks the one it
applies. With no file at all, it says what to write.

A manager earns a place here only when its *explicitly installed* set can be
listed, and only once its real output has been captured by running it. Every
one but `flatpak`, `code` and `codium` (which need a privileged container or a
display) is tested weekly against the real tool in its own container: vow
installs a package with its own commands, reads it back, searches, reports what
is outdated, and removes it.

Version managers are converged by their **active** version rather than their
installed set — `mise ls --current`, `asdf current`, `ghcup list -c set` — since
having a toolchain unpacked somewhere is not the same as using it. volta
cannot uninstall a Node runtime, so an undeclared one is left in place with a
warning rather than planned for removal.

Some obvious candidates are deliberately absent, and the reasons are worth
stating because they are the same handful over and over:

- **Dependencies cannot be told from choices.** `rpm`, `haxelib`, `nimble`,
  `conda`, `cabal` and `tlmgr` list everything their listing can reach, with no
  flag to separate what you asked for — the same reason vow uses `pacman -Qe`
  rather than `pacman -Q`. A manager whose listing cannot make that distinction
  would have vow offer to uninstall dependencies.
- **Nothing can be listed at all.** `deno` has no installed-set query.
- **The tool's own file is already the declaration.** `zinit`, `lazy.nvim`,
  `sheldon` and `nix profile` are declared in a file you already keep; vow would
  be a second source of truth for the same thing.
- **The command cannot be built as an argument vector.** `julia`'s package
  operations go through a script string, which is exactly the shape vow refuses
  to construct.
- **It needs something vow cannot ask for.** `gh extension` needs auth; `cpan`
  needs interactive configuration.

`apk` is the one exception to the "explicitly installed" rule: Alpine draws no
line between a package you asked for and one pulled in as a dependency, so
`generate-config` there records the base system too.

## Environment

| Variable | Effect |
|-|-|
| `VOW_CONFIG_DIR` | graph location (default `$XDG_CONFIG_HOME/vow`, else `~/.config/vow`); `--path` overrides it for one run |
| `VOW_HOST` | override the detected hostname |
| `VOW_SUDO` | privilege elevator (default `sudo`; set empty to never escalate) |
| `NO_COLOR` | disable colour, and with it the live boxes of a switch |
| `LC_ALL`, `LC_CTYPE`, `LANG` | a UTF-8 locale draws boxes with box-drawing characters; otherwise `+-\|` |
| `TERM` | `linux` or `dumb` draws them with `+-\|` whatever the locale |

vow keeps its own state -- the record of links it placed and the logs of failed
managers -- in `~/.local/share/vow`, and nowhere else.
