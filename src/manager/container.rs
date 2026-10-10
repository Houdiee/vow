//! Each manager against the real tool, in its own container: vow's own
//! commands install, list, search, report outdated and remove a package.
//!
//!     VOW_CONTAINERS=1 cargo test container          # every manager
//!     VOW_CONTAINERS=1 cargo test container::apt     # one
//!
//! Needs docker or podman. flatpak is absent: it needs a privileged container.

use super::get;
use crate::exec::Invocation;
use crate::packages::PackageSpec;
use std::process::{Command, Output};

#[derive(Default)]
struct Recipe {
    id: &'static str,
    image: &'static str,
    /// Installs the manager itself, where the image lacks it.
    setup: &'static str,
    /// Prefixed to every step, for a manager that lives off the default PATH.
    env: String,
    /// What vow installs, as declared: `name` or `name version`.
    package: &'static str,
    /// How the listing names it, when not by the package's name.
    listed: &'static str,
    search: &'static str,
    /// Installs an older version, so `outdated` has something to report.
    stale: &'static str,
    /// The package `outdated` must name; empty checks only that its output parses.
    behind: &'static str,
}

/// A container kept running for one recipe, removed when dropped.
struct Container {
    engine: &'static str,
    id: String,
}

impl Container {
    fn start(engine: &'static str, image: &str) -> Self {
        let output = Command::new(engine)
            .args([
                "run",
                "-d",
                "--entrypoint",
                "sh",
                image,
                "-c",
                "sleep 86400",
            ])
            .output()
            .unwrap_or_else(|error| panic!("could not start {image}: {error}"));
        assert!(
            output.status.success(),
            "could not start {image}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self {
            engine,
            id: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        }
    }

    fn sh(&self, env: &str, script: &str) -> Output {
        Command::new(self.engine)
            .args(["exec", &self.id, "sh", "-c", &format!("{env} {script}")])
            .output()
            .expect("docker exec")
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new(self.engine)
            .args(["rm", "-f", &self.id])
            .output();
    }
}

fn runtime() -> Option<&'static str> {
    ["docker", "podman"].into_iter().find(|engine| {
        Command::new(engine)
            .arg("info")
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

/// Commands as a shell line, without sudo: a container runs as its own user.
fn shell(commands: &[Invocation]) -> String {
    commands
        .iter()
        .map(|command| {
            Invocation {
                needs_root: false,
                ..command.clone()
            }
            .display()
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn verify(recipe: Recipe) {
    let id = recipe.id;
    if std::env::var_os("VOW_CONTAINERS").is_none() {
        eprintln!("skipped {id}: set VOW_CONTAINERS=1 to run against containers");
        return;
    }
    let Some(engine) = runtime() else {
        eprintln!("skipped {id}: no usable container runtime");
        return;
    };
    let manager = get(id).unwrap_or_else(|| panic!("`{id}` is not registered"));
    let package = PackageSpec::parse(recipe.package).expect("a package");
    let listed = if recipe.listed.is_empty() {
        package.name.as_str()
    } else {
        recipe.listed
    };
    let find = |specs: &[PackageSpec]| {
        specs
            .iter()
            .find(|spec| manager.canonical(&spec.name) == manager.canonical(listed))
            .cloned()
    };

    let container = Container::start(engine, recipe.image);
    let run = |script: &str| container.sh(&recipe.env, script);
    if !recipe.setup.is_empty() {
        let setup = run(recipe.setup);
        assert!(
            setup.status.success(),
            "{id}: setup failed:\n{}",
            text(&setup)
        );
    }

    let install = shell(&manager.install_commands(std::slice::from_ref(&package)));
    let installed = run(&install);
    assert!(
        installed.status.success(),
        "{id}: `{install}` failed:\n{}",
        text(&installed)
    );

    let list = shell(&[manager.list_command()]);
    let listing = String::from_utf8_lossy(&run(&list).stdout).into_owned();
    let parsed = manager.parse_list(&listing);
    let Some(found) = find(&parsed) else {
        panic!("{id}: `{list}` did not show `{listed}`:\n{listing}\nparsed: {parsed:?}");
    };
    // A drifting parser picks up decoration, which would not round-trip.
    for spec in &parsed {
        assert!(
            PackageSpec::parse(&spec.to_string()).is_ok(),
            "{id}: parsed `{spec}`, which the declaration grammar rejects"
        );
    }
    eprintln!("{id}: installed and listed `{found}`");

    if !recipe.search.is_empty() {
        let search = shell(&[manager.search_command(recipe.search).expect("a search")]);
        let hits = text(&run(&search));
        assert!(
            hits.contains(recipe.search),
            "{id}: `{search}` found nothing:\n{hits}"
        );
    }

    let uninstall = shell(&manager.uninstall_commands(std::slice::from_ref(&found)));
    let removed = run(&uninstall);
    assert!(
        removed.status.success(),
        "{id}: `{uninstall}` failed:\n{}",
        text(&removed)
    );
    let after = String::from_utf8_lossy(&run(&list).stdout).into_owned();
    assert!(
        find(&manager.parse_list(&after)).is_none(),
        "{id}: `{listed}` is still listed after `{uninstall}`:\n{after}"
    );
    eprintln!("{id}: removed `{listed}`");

    if !recipe.stale.is_empty() {
        let stale = run(recipe.stale);
        assert!(
            stale.status.success(),
            "{id}: `{}` failed:\n{}",
            recipe.stale,
            text(&stale)
        );
        let outdated = shell(&[manager.outdated_command().expect("an outdated command")]);
        let report = String::from_utf8_lossy(&run(&outdated).stdout).into_owned();
        let behind = manager.parse_outdated(&report);
        if recipe.behind.is_empty() {
            assert!(
                behind.iter().all(|spec| spec.version.is_some()),
                "{id}: `{outdated}` parsed without versions:\n{report}"
            );
        } else {
            assert!(
                behind.iter().any(|spec| spec.name == recipe.behind),
                "{id}: `{outdated}` did not report `{}`:\n{report}",
                recipe.behind
            );
        }
    }
}

#[test]
fn apt() {
    verify(Recipe {
        id: "apt",
        image: "debian:stable-slim",
        setup: "apt-get update -qq",
        package: "ripgrep",
        search: "ripgrep",
        stale: "apt-get update -qq",
        ..Recipe::default()
    });
}

#[test]
fn pacman() {
    verify(Recipe {
        id: "pacman",
        image: "archlinux:latest",
        setup: "pacman -Syu --noconfirm",
        package: "ripgrep",
        search: "ripgrep",
        // -Sy, not -Syu: refresh the index without upgrading, or nothing is behind.
        stale: "pacman -Sy --noconfirm",
        ..Recipe::default()
    });
}

#[test]
fn gem() {
    verify(Recipe {
        id: "gem",
        image: "ruby:slim",
        package: "tilt",
        search: "tilt",
        stale: "gem install --no-document tilt -v 2.0.11",
        behind: "tilt",
        ..Recipe::default()
    });
}

#[test]
fn pipx() {
    verify(Recipe {
        id: "pipx",
        image: "python:slim",
        setup: "pip install -q pipx",
        env: "export PATH=/root/.local/bin:$PATH;".into(),
        package: "cowsay",
        ..Recipe::default()
    });
}

#[test]
fn composer() {
    verify(Recipe {
        id: "composer",
        image: "composer:latest",
        package: "psr/log",
        search: "psr/log",
        stale: "composer global require --no-interaction psr/log:1.1.4",
        behind: "psr/log",
        ..Recipe::default()
    });
}

#[test]
fn npm() {
    verify(Recipe {
        id: "npm",
        image: "node:slim",
        package: "is-odd",
        search: "is-odd",
        stale: "npm install -g is-odd@2.0.0",
        behind: "is-odd",
        ..Recipe::default()
    });
}

#[test]
fn pnpm() {
    verify(Recipe {
        id: "pnpm",
        image: "node:slim",
        setup: "npm install -g pnpm && mkdir -p /root/.pnpm/bin",
        // pnpm refuses a global install until PNPM_HOME is on PATH.
        env: "export PNPM_HOME=/root/.pnpm PATH=/root/.pnpm/bin:$PATH;".into(),
        package: "is-odd",
        ..Recipe::default()
    });
}

#[test]
fn bun() {
    verify(Recipe {
        id: "bun",
        image: "oven/bun:slim",
        package: "typescript",
        ..Recipe::default()
    });
}

#[test]
fn yarn() {
    verify(Recipe {
        id: "yarn",
        image: "node:slim",
        setup: "npm install -g yarn",
        // `yarn global list` shows only packages with a binary.
        package: "prettier",
        ..Recipe::default()
    });
}

#[test]
fn cargo() {
    verify(Recipe {
        id: "cargo",
        image: "rust:slim",
        package: "cargo-expand",
        search: "ripgrep",
        ..Recipe::default()
    });
}

#[test]
fn dotnet() {
    verify(Recipe {
        id: "dotnet",
        image: "mcr.microsoft.com/dotnet/sdk:latest",
        package: "csharpier",
        search: "csharpier",
        ..Recipe::default()
    });
}

#[test]
fn apk() {
    verify(Recipe {
        id: "apk",
        image: "alpine:latest",
        setup: "apk update",
        package: "ripgrep",
        search: "ripgrep",
        ..Recipe::default()
    });
}

#[test]
fn xbps() {
    verify(Recipe {
        id: "xbps",
        image: "ghcr.io/void-linux/void-glibc-full:latest",
        setup: "xbps-install -Syu xbps && xbps-install -S",
        package: "ripgrep",
        search: "ripgrep",
        ..Recipe::default()
    });
}

#[test]
fn dnf() {
    verify(Recipe {
        id: "dnf",
        image: "fedora:latest",
        package: "ripgrep",
        search: "ripgrep",
        ..Recipe::default()
    });
}

#[test]
fn zypper() {
    verify(Recipe {
        id: "zypper",
        image: "opensuse/tumbleweed",
        package: "ripgrep",
        search: "ripgrep",
        ..Recipe::default()
    });
}

// Homebrew on Linux: the formulae and output match macOS.
#[test]
fn brew() {
    verify(Recipe {
        id: "brew",
        image: "homebrew/brew:latest",
        package: "hello",
        search: "hello",
        ..Recipe::default()
    });
}

// Alpine names it `luarocks-5.4`.
#[test]
fn luarocks() {
    verify(Recipe {
        id: "luarocks",
        image: "alpine:latest",
        setup: "apk add --no-cache luarocks5.4 lua5.4-dev build-base \
                && ln -sf /usr/bin/luarocks-5.4 /usr/local/bin/luarocks",
        package: "penlight",
        ..Recipe::default()
    });
}

#[test]
fn pip() {
    verify(Recipe {
        id: "pip",
        image: "python:slim",
        package: "cowsay",
        stale: "pip install --user cowsay==5.0",
        behind: "cowsay",
        ..Recipe::default()
    });
}

#[test]
fn uv() {
    verify(Recipe {
        id: "uv",
        image: "ghcr.io/astral-sh/uv:debian",
        package: "cowsay",
        ..Recipe::default()
    });
}

#[test]
fn opam() {
    verify(Recipe {
        id: "opam",
        image: "ocaml/opam",
        package: "ocamlfind",
        search: "ocamlfind",
        ..Recipe::default()
    });
}

#[test]
fn dart_pub() {
    verify(Recipe {
        id: "pub",
        image: "dart:stable",
        package: "http",
        ..Recipe::default()
    });
}

#[test]
fn raco() {
    verify(Recipe {
        id: "raco",
        image: "racket/racket:latest",
        package: "rackunit-lib",
        ..Recipe::default()
    });
}

#[test]
fn coursier() {
    verify(Recipe {
        id: "coursier",
        image: "eclipse-temurin:21",
        setup: "curl -fsSL https://github.com/coursier/launchers/raw/master/coursier \
                -o /usr/local/bin/cs && chmod +x /usr/local/bin/cs",
        package: "scalafmt",
        ..Recipe::default()
    });
}

// vow reads and removes binaries in this machine's GOBIN, so the container uses it too.
#[test]
fn go() {
    let gobin = get("go").expect("registered").list_command().args[2].clone();
    verify(Recipe {
        id: "go",
        image: "golang:latest",
        env: format!("export GOBIN={gobin};"),
        package: "github.com/rakyll/hey v0.1.5",
        ..Recipe::default()
    });
}

#[test]
fn rustup() {
    verify(Recipe {
        id: "rustup",
        image: "rust:slim",
        package: "1.80.0",
        ..Recipe::default()
    });
}

#[test]
fn mise() {
    verify(Recipe {
        id: "mise",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl git ca-certificates \
                && curl -fsSL https://mise.run | sh",
        env: "export PATH=$HOME/.local/bin:$PATH;".into(),
        package: "node 22.11.0",
        ..Recipe::default()
    });
}

#[test]
fn asdf() {
    verify(Recipe {
        id: "asdf",
        image: "golang:latest",
        setup: "go install github.com/asdf-vm/asdf/cmd/asdf@latest",
        env: "export PATH=/go/bin:$PATH;".into(),
        package: "nodejs 22.11.0",
        ..Recipe::default()
    });
}

#[test]
fn pyenv() {
    verify(Recipe {
        id: "pyenv",
        image: "python:slim",
        setup: "apt-get update -qq && apt-get install -y -qq git curl build-essential \
                libssl-dev zlib1g-dev libbz2-dev libffi-dev libreadline-dev libsqlite3-dev \
                && curl -fsSL https://pyenv.run | bash",
        env: "export PYENV_ROOT=$HOME/.pyenv PATH=$HOME/.pyenv/bin:$PATH;".into(),
        package: "3.12.8",
        ..Recipe::default()
    });
}

#[test]
fn volta() {
    verify(Recipe {
        id: "volta",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates \
                && curl -fsSL https://get.volta.sh | bash -s -- --skip-setup \
                && volta install node",
        env: "export VOLTA_HOME=$HOME/.volta PATH=$HOME/.volta/bin:$PATH;".into(),
        // A runtime can't be uninstalled (`Volta::keeps`), so a package is the one
        // removed; it needs a runtime first.
        package: "typescript 5.6.3",
        ..Recipe::default()
    });
}

#[test]
fn ghcup() {
    verify(Recipe {
        id: "ghcup",
        image: "haskell:latest",
        setup: "curl --proto '=https' --tlsv1.2 -sSf https://get-ghcup.haskell.org \
                | BOOTSTRAP_HASKELL_NONINTERACTIVE=1 sh",
        env: "export PATH=$HOME/.ghcup/bin:$PATH;".into(),
        package: "ghc 9.8.2",
        ..Recipe::default()
    });
}

#[test]
fn elan() {
    verify(Recipe {
        id: "elan",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates git \
                && curl -fsSL https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh \
                | sh -s -- -y --default-toolchain none",
        env: "export PATH=$HOME/.elan/bin:$PATH;".into(),
        package: "leanprover/lean4:v4.34.1",
        ..Recipe::default()
    });
}

#[test]
fn zvm() {
    verify(Recipe {
        id: "zvm",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates git \
                && curl -fsSL https://raw.githubusercontent.com/tristanisham/zvm/master/install.sh | bash",
        env: "export PATH=$HOME/.zvm/self:$HOME/.zvm/bin:$PATH;".into(),
        package: "0.13.0",
        ..Recipe::default()
    });
}

#[test]
fn bob() {
    verify(Recipe {
        id: "bob",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates unzip \
                && curl -fsSL https://github.com/MordechaiHadad/bob/releases/latest/download/bob-linux-x86_64.zip \
                -o /tmp/b.zip && unzip -q -o /tmp/b.zip -d /tmp/b \
                && install -m755 /tmp/b/bob-linux-x86_64/bob /usr/local/bin/bob",
        package: "0.10.4",
        listed: "v0.10.4",
        ..Recipe::default()
    });
}

// code-server stands in for `code` and `codium`, which ship no image.
#[test]
fn code_server() {
    verify(Recipe {
        id: "code-server",
        image: "codercom/code-server:latest",
        package: "esbenp.prettier-vscode",
        ..Recipe::default()
    });
}

#[test]
fn krew() {
    verify(Recipe {
        id: "krew",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates git \
                && curl -fsSL https://github.com/kubernetes-sigs/krew/releases/latest/download/krew-linux_amd64.tar.gz \
                | tar xz -C /tmp && KREW_ROOT=/tmp/krew /tmp/krew-linux_amd64 install krew \
                && cp /tmp/krew/bin/kubectl-krew /usr/local/bin/",
        env: "export KREW_ROOT=/tmp/krew;".into(),
        package: "ctx",
        search: "ctx",
        ..Recipe::default()
    });
}

#[test]
fn pixi() {
    verify(Recipe {
        id: "pixi",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates \
                && curl -fsSL https://pixi.sh/install.sh | sh",
        env: "export PATH=$HOME/.pixi/bin:$PATH;".into(),
        package: "jq 1.7.1",
        ..Recipe::default()
    });
}

#[test]
fn fisher() {
    verify(Recipe {
        id: "fisher",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq fish curl git ca-certificates \
                && fish -c 'curl -sL https://raw.githubusercontent.com/jorgebucaran/fisher/main/functions/fisher.fish \
                | source && fisher install jorgebucaran/fisher'",
        package: "jorgebucaran/nvm.fish",
        ..Recipe::default()
    });
}

// The server must be up before the CLI answers.
#[test]
fn ollama() {
    verify(Recipe {
        id: "ollama",
        image: "ollama/ollama:latest",
        setup: "(ollama serve >/tmp/serve.log 2>&1 &) ; sleep 8",
        package: "smollm:135m",
        ..Recipe::default()
    });
}

#[test]
fn yazi() {
    verify(Recipe {
        id: "yazi",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates git unzip \
                && curl -fsSL https://github.com/sxyazi/yazi/releases/latest/download/yazi-x86_64-unknown-linux-gnu.zip \
                -o /tmp/y.zip && unzip -q -o /tmp/y.zip -d /tmp/y \
                && install -m755 /tmp/y/yazi-x86_64-unknown-linux-gnu/ya /usr/local/bin/ya",
        package: "yazi-rs/plugins:full-border",
        ..Recipe::default()
    });
}

#[test]
fn soar() {
    verify(Recipe {
        id: "soar",
        image: "debian:stable-slim",
        setup: "apt-get update -qq && apt-get install -y -qq curl ca-certificates \
                && curl -fsSL https://raw.githubusercontent.com/pkgforge/soar/main/install.sh | sh",
        env: "export PATH=$HOME/.local/bin:$PATH;".into(),
        package: "jq",
        search: "jq",
        ..Recipe::default()
    });
}
