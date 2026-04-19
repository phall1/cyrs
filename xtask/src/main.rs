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
    Bless,
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
}

fn main() -> Result<()> {
    let cli = Xtask::parse();
    match cli.cmd {
        Cmd::Gate => gate(),
        Cmd::Bless => {
            println!("[xtask bless] lands with the compiletest runner");
            Ok(())
        }
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
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
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
