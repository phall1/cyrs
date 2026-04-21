//! `cargo xtask check-changelogs` — verify every workspace crate has a
//! well-formed `CHANGELOG.md` (spec 0001 §18, bead cy-urk).
//!
//! Runs as part of the release gate and is cheap enough to run in CI on
//! every PR.  The check is intentionally minimal so it catches the
//! structural mistake ("new crate landed without a changelog") without
//! imposing a strict content schema that would force authors to edit the
//! file for every change.
//!
//! # Checks
//!
//! For every directory under `crates/`:
//!
//! 1. `CHANGELOG.md` exists at `crates/<name>/CHANGELOG.md`.
//! 2. The file contains an `## [Unreleased]` heading (case-sensitive).
//!
//! The workspace-root `CHANGELOG.md` is checked with the same rule.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Heading that every changelog must carry so future release tooling can
/// pivot `[Unreleased]` into a versioned section.
const UNRELEASED_MARKER: &str = "## [Unreleased]";

pub fn run() -> Result<()> {
    let workspace_root = workspace_root()?;
    let mut failures: Vec<String> = Vec::new();

    let root_changelog = workspace_root.join("CHANGELOG.md");
    if let Err(err) = validate_file(&root_changelog, "<workspace root>") {
        failures.push(err.to_string());
    }

    let crates_dir = workspace_root.join("crates");
    let mut crate_paths: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
        .with_context(|| format!("reading {}", crates_dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_paths.sort();

    for crate_path in &crate_paths {
        let changelog = crate_path.join("CHANGELOG.md");
        let name = crate_path.file_name().map_or_else(
            || "<unknown>".to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        if let Err(err) = validate_file(&changelog, &name) {
            failures.push(err.to_string());
        }
    }

    if failures.is_empty() {
        println!(
            "check-changelogs: OK ({} crate(s) + workspace root)",
            crate_paths.len()
        );
        Ok(())
    } else {
        eprintln!("check-changelogs: FAIL");
        for f in &failures {
            eprintln!("  {f}");
        }
        bail!("{} changelog(s) missing or malformed", failures.len());
    }
}

fn validate_file(path: &Path, owner: &str) -> Result<()> {
    if !path.exists() {
        bail!("{owner}: {} is missing (spec §18)", path.display());
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if !content.contains(UNRELEASED_MARKER) {
        bail!(
            "{owner}: {} must contain a `{UNRELEASED_MARKER}` section (keep-a-changelog)",
            path.display()
        );
    }
    Ok(())
}

/// The workspace root, i.e. the directory containing the outer `Cargo.toml`.
/// This xtask binary is always invoked from that directory because the
/// workspace resolver runs `cargo run -p xtask` from there.
fn workspace_root() -> Result<PathBuf> {
    let here = std::env::current_dir().context("reading current dir")?;
    if here.join("Cargo.toml").exists() && here.join("crates").is_dir() {
        Ok(here)
    } else {
        bail!(
            "xtask check-changelogs must be run from the workspace root \
             (expected Cargo.toml + crates/ at {})",
            here.display()
        )
    }
}
