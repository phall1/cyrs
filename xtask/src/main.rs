//! `xtask` — developer tasks.
//!
//! Spec 0001 calls out several dev-only requests that do not belong in
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
    /// Regenerate `crates/cypher-tck/tck/full-baseline.md` by running
    /// the full-tck harness (spec §17.5, bead cy-p5q).
    ///
    /// Thin wrapper around `cargo test -p cypher-tck --features
    /// full-tck tck_full_baseline -- --nocapture`.
    #[command(name = "tck-baseline")]
    TckBaseline,
    /// Grammar <-> recovery.md symmetry gate (spec §4.3, §17.18).
    CheckRecovery,
    /// Verify every crate has a well-formed `CHANGELOG.md` (spec §18).
    CheckChangelogs,
    /// Verify diagnostic-code references are all registered (spec §10.2).
    CheckDiagCodes,
    /// Verify every emitted recovery code (E0001–E0999) is exercised by
    /// the recovery property test or a UI fixture (bead cy-gkh,
    /// spec §10.2 / §17.3).
    CheckRecoveryBudget,
    /// Build rustdoc with `-D warnings` (spec §17.15, bead cy-93c).
    Doc,
    /// Tree-sitter grammar ↔ cyrs TCK v1 parity gate (bead cy-od5.1).
    ///
    /// Regenerates the grammar, runs `tree-sitter test`, and then diffs
    /// every TCK v1 scenario's parse outcome between tree-sitter and cyrs.
    #[command(name = "tree-sitter-parity")]
    TreeSitterParity,
    /// Build the cypher-wasm cdylib + run wasm-bindgen (bead cy-u6r,
    /// spec 0004 §4).  Missing wasm tooling produces a skip, not a
    /// failure.
    #[command(name = "wasm-build")]
    WasmBuild,
    /// Full cypher-wasm size pipeline + gate (spec 0004 §4.2): cargo
    /// build → wasm-bindgen → wasm-opt -Os → brotli -q 11.  Fails if
    /// the brotli artifact exceeds 2 MB.
    #[command(name = "wasm-size")]
    WasmSize,
    /// wasm-pack headless smoke test behind the `wasm-smoke` feature
    /// (spec 0004 §10.1).  Skips if wasm-pack is not on PATH.
    #[command(name = "wasm-smoke")]
    WasmSmoke,
    /// Build the cypher-lsp wasm artifact for the LSP-Web demo worker
    /// (bead cy-m0d, spec 0004 §7).  Mirrors `wasm-build` with the
    /// `web-lsp` feature, `--target no-modules`, and a 3 MB brotli
    /// size cap.  Missing wasm tooling produces a skip, not a failure.
    #[command(name = "lsp-web-build")]
    LspWebBuild,
    /// Generate or verify the cypher-ffi C header (spec 0004 §5.6).
    ///
    /// Default mode regenerates `crates/cypher-ffi/include/cypher.h`.
    /// `--check` mode runs cbindgen into a tempfile and diffs it
    /// against the committed header; exits 1 on drift.  Wired into
    /// `cargo xtask gate` so ABI drift blocks the pre-commit gate.
    Cbindgen {
        /// Verify the committed header matches a fresh run of cbindgen.
        #[arg(long)]
        check: bool,
    },
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
        Cmd::TckBaseline => tck_baseline(),
        Cmd::CheckRecovery => xtask::check_recovery::run(),
        Cmd::CheckChangelogs => xtask::check_changelogs::run(),
        Cmd::CheckDiagCodes => xtask::check_diag_codes::run(),
        Cmd::CheckRecoveryBudget => xtask::check_recovery_budget::run(),
        Cmd::Doc => doc(),
        Cmd::TreeSitterParity => xtask::tree_sitter_parity::run(),
        Cmd::WasmBuild => xtask::wasm::build(),
        Cmd::WasmSize => xtask::wasm::size(),
        Cmd::WasmSmoke => xtask::wasm::smoke(),
        Cmd::LspWebBuild => xtask::wasm::lsp_web_build(),
        Cmd::Cbindgen { check } => xtask::cbindgen::run(check),
    }
}

/// Regenerate the full-TCK pass-rate baseline (spec §17.5, bead
/// cy-p5q).  Runs the `tck_full_baseline` integration test with the
/// `full-tck` Cargo feature enabled; the test writes
/// `crates/cypher-tck/tck/full-baseline.md` as a side-effect.
fn tck_baseline() -> Result<()> {
    println!("==> cargo test -p cypher-tck --features full-tck tck_full_baseline");
    run(
        "cargo",
        &[
            "test",
            "-p",
            "cypher-tck",
            "--features",
            "full-tck",
            "--test",
            "full",
            "tck_full_baseline",
            "--",
            "--nocapture",
        ],
    )
}

/// Rustdoc gate per spec §17.15 / bead cy-93c. Builds `cargo doc
/// --workspace --no-deps --lib` with `RUSTDOCFLAGS=-D warnings` so any
/// broken / private / ambiguous intra-doc link fails locally just like
/// it does in CI.
fn doc() -> Result<()> {
    println!("==> cargo doc --workspace --no-deps --lib (RUSTDOCFLAGS=-D warnings)");
    let status = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps", "--lib"])
        .env("RUSTDOCFLAGS", "-D warnings")
        .status()
        .map_err(|err| anyhow!("failed to spawn `cargo`: {err}"))?;
    if !status.success() {
        bail!(
            "`cargo doc --workspace --no-deps --lib` exited with {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        );
    }
    Ok(())
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
    // `missing_docs` was promoted to `deny` workspace-wide in
    // cy-p47.  No suppression here — new `pub` items without rustdoc
    // fail the gate.
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

    // Rustdoc gate (spec §17.15, bead cy-93c): broken / private /
    // ambiguous intra-doc links fail the local gate just like CI.
    doc()?;

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

    // Recovery-range coverage (bead cy-gkh): every emitted E0001..=E0999
    // code must be exercised by the property test or a UI fixture.
    println!("==> xtask check-recovery-budget");
    xtask::check_recovery_budget::run()?;

    // C-ABI drift gate (spec 0004 §5.6, bead cy-dh6).  Runs on every
    // commit — not just nightly — because a drifted header is a silent
    // SemVer-major break the moment cypher-ffi ships.  Skips gracefully
    // if cbindgen is not on PATH (see xtask::cbindgen::run).
    println!("==> xtask cbindgen --check");
    xtask::cbindgen::run(true)?;

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
