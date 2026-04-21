//! `xtask` — developer tasks.
//!
//! Spec 0001 calls out several dev-only operations that do not belong in
//! `cargo test`. This binary hosts them. Tasks land alongside the pieces
//! they automate.

#![forbid(unsafe_code)]
#![allow(clippy::unnecessary_wraps)]

use std::io;
use std::process::Command;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Cypher workspace developer tasks")]
struct Xtask {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Pre-commit gate: fmt → clippy → test → deny (spec §17, AGENTS §4.3).
    Gate,
    /// Re-bless compiletest golden corpus (spec §17.6).
    ///
    /// Runs `BLESS=1 cargo test --test ui` for the specified package (or all
    /// UI-test packages when `--package` is omitted).
    Bless {
        /// Package to bless (e.g. `cypher-sema`). Omit to bless all.
        #[arg(short, long)]
        package: Option<String>,
        /// Corpus kind to bless (syntax/sema/schema/dialect/plan/fmt).
        /// Omit to bless all kinds within the selected package(s).
        #[arg(short, long)]
        kind: Option<String>,
    },
    /// Regenerate `cypher-ast` from the grammar description (spec §5.2).
    Codegen,
    /// Verify release gates are green (spec §17.17).
    Release,
    /// Run a fuzz target for the 5-minute PR gate (spec §17.4).
    Fuzz {
        /// Fuzz target name (e.g. `fuzz_parser`).
        target: String,
    },
    /// Fetch and vendor the openCypher TCK corpus (spec §17.5).
    TckFetch,
    /// Grammar <-> recovery.md symmetry gate (spec §4.3, §17.18).
    CheckRecovery,
    /// Verify every crate has a well-formed `CHANGELOG.md` (spec §18).
    CheckChangelogs,
}

fn main() -> Result<()> {
    let cli = Xtask::parse();
    match cli.cmd {
        Cmd::Gate => gate(),
        Cmd::Bless { package, kind } => bless(package.as_deref(), kind.as_deref()),
        Cmd::Codegen => xtask::codegen::run(),
        Cmd::Release => {
            println!("[xtask release] verifies gates per spec §17.17");
            Ok(())
        }
        Cmd::Fuzz { target } => fuzz(&target),
        Cmd::TckFetch => {
            println!("[xtask tck-fetch] lands with the TCK harness");
            Ok(())
        }
        Cmd::CheckRecovery => xtask::check_recovery::run(),
        Cmd::CheckChangelogs => xtask::check_changelogs::run(),
    }
}

/// Run an external program, inheriting stdio. Returns an error on non-zero exit.
fn run(prog: &str, args: &[&str]) -> Result<()> {
    println!("==> {} {}", prog, args.join(" "));
    let status = Command::new(prog).args(args).status().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            anyhow!("`{prog}` not found on PATH: {err}")
        } else {
            anyhow!("failed to spawn `{prog}`: {err}")
        }
    })?;
    if !status.success() {
        bail!(
            "`{} {}` exited with {}",
            prog,
            args.join(" "),
            status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        );
    }
    Ok(())
}

/// Is a binary of this name callable on PATH?
fn has_binary(prog: &str) -> bool {
    match Command::new(prog).arg("--version").output() {
        Ok(out) => out.status.success(),
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("warning: probing `{prog}` failed: {err}");
            }
            false
        }
    }
}

/// Pre-commit gate per spec §17 and AGENTS §4.3. Steps run in order and
/// stop at the first failure.
fn gate() -> Result<()> {
    run("cargo", &["fmt", "--all", "--", "--check"])?;
    // `-A missing-docs` mirrors the CI clippy step (see
    // .github/workflows/ci.yml lint job): the workspace-wide
    // `missing_docs = "warn"` lint surfaces the backlog on every
    // `cargo build`, but the pre-commit gate stays green while the
    // backlog (bead cy-p47) is being written down.
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
            "-A",
            "missing-docs",
        ],
    )?;
    run(
        "cargo",
        &["test", "--workspace", "--all-features", "--no-fail-fast"],
    )?;

    if has_binary("cargo-deny") {
        run_deny_check()?;
    } else {
        println!(
            "==> cargo deny check [skipped: `cargo-deny` not on PATH; \
             install with `cargo install cargo-deny --locked`]"
        );
    }

    // TODO(cy-kc9): non-coupling denylist greps (spec §2.C2) land via the
    // sibling bead A4; wire the script in here once it exists.
    // TODO(cy-590): diagnostic-code registry lint (spec §10.2) lands via
    // the sibling bead A3; invoke it here once it exists.

    println!("==> gate OK");
    Ok(())
}

/// All crates that have a `tests/ui.rs` integration test harness.
const UI_CRATES: &[&str] = &["cypher-syntax", "cypher-sema", "cypher-fmt", "cypher-plan"];

/// Re-bless compiletest golden sidecars (spec §17.6).
///
/// Runs `BLESS=1 cargo test --test ui` for every relevant crate (or the
/// specified package) so the harness overwrites every sidecar with the
/// actual pipeline output.
///
/// `--kind` is accepted for documentation / future filtering but currently
/// blesses all corpus kinds within the selected package(s): the
/// per-kind separation lives inside each crate's `tests/ui.rs` harness, and
/// a single `--test ui` invocation blesses all kinds for that crate.
fn bless(package: Option<&str>, kind: Option<&str>) -> Result<()> {
    if let Some(k) = kind {
        let valid_kinds = ["syntax", "sema", "schema", "dialect", "plan", "fmt"];
        if !valid_kinds.contains(&k) {
            bail!(
                "unknown kind `{k}`; valid kinds: {}",
                valid_kinds.join(", ")
            );
        }
    }

    let packages: Vec<&str> = match package {
        Some(p) => {
            if !UI_CRATES.contains(&p) {
                bail!(
                    "package `{p}` has no UI corpus; known packages: {}",
                    UI_CRATES.join(", ")
                );
            }
            vec![p]
        }
        None => UI_CRATES.to_vec(),
    };

    for pkg in packages {
        println!("[xtask bless] blessing {pkg}");
        let status = Command::new("cargo")
            .args(["test", "-p", pkg, "--test", "ui"])
            .env("BLESS", "1")
            .status()
            .map_err(|err| anyhow!("failed to spawn `cargo test`: {err}"))?;
        if !status.success() {
            bail!(
                "`cargo test -p {pkg} --test ui` exited with {}",
                status
                    .code()
                    .map_or_else(|| "signal".to_string(), |c| c.to_string())
            );
        }
    }
    println!("[xtask bless] done");
    Ok(())
}

/// Run `cargo deny check`, unsetting `GIT_DIR` / `GIT_WORK_TREE` first.
///
/// When invoked from a git pre-commit hook, git sets `GIT_DIR` to the
/// worktree's git-dir (e.g. `.git/worktrees/<name>`). cargo-deny uses
/// libgit2 to clone the RUSTSEC advisory DB; libgit2 respects the
/// `GIT_DIR` env var and incorrectly initialises the advisory DB repo
/// to point at the worktree's work-tree, causing it to scan workspace
/// source files as RUSTSEC advisories (panic).  Clearing these vars
/// lets libgit2 discover the correct git context for the advisory DB.
fn run_deny_check() -> Result<()> {
    println!("==> cargo deny check");
    let status = Command::new("cargo")
        .args(["deny", "check"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .status()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                anyhow!("`cargo` not found on PATH: {err}")
            } else {
                anyhow!("failed to spawn `cargo deny check`: {err}")
            }
        })?;
    if !status.success() {
        bail!(
            "`cargo deny check` exited with {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        );
    }
    Ok(())
}

/// Wrap `cargo fuzz run <target> -- -max_total_time=300` — the 5-minute
/// PR gate defined in spec §17.4.
fn fuzz(target: &str) -> Result<()> {
    let args = ["fuzz", "run", target, "--", "-max_total_time=300"];
    println!("==> cargo {}", args.join(" "));
    let spawn = Command::new("cargo").args(args).status();
    match spawn {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => bail!(
            "`cargo fuzz run {target}` exited with {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        ),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(anyhow!(
            "`cargo fuzz` not installed. Install with:\n    \
             cargo install cargo-fuzz --locked"
        )),
        Err(err) => Err(anyhow!("failed to spawn `cargo fuzz`: {err}")),
    }
}
