//! `cargo xtask check-diag-codes` — registry consistency gate
//! (spec §10.2, §17.17, bead cy-wnm).
//!
//! Walks `crates/cypher-diag/src/codes.rs` for the authoritative set of
//! diagnostic codes, then grep-walks all workspace `.rs` files for
//! `Code::…` references.  Fails the gate if any referenced code is not
//! registered — a misspelled variant otherwise compiles-fine today
//! (the enum is the same symbol) but would be a drift risk the day we
//! gain any code-table generation.
//!
//! # Checks
//!
//! 1. **Orphan references**: any `Code::E0001` / `Code::W6000` / `Code::N8000`
//!    identifier referenced in `crates/**/*.rs` must exist in the
//!    `Code` enum defined in `codes.rs`.  This catches typos and
//!    half-merged renames.
//! 2. **Unused registry entries**: reported as info only.  Some codes are
//!    reserved for features that have landed the code reservation but
//!    not the emitter yet; we do not fail on this, just list them so
//!    a reviewer can decide.
//!
//! # Why grep instead of building the crate
//!
//! This gate runs in CI before the full test matrix.  Parsing .rs via
//! regex is ugly but keeps the dependency surface small: the
//! alternative (load cypher-diag via syn / rust-analyzer) would make
//! this gate the slowest step in the pipeline for no extra rigour on
//! what we actually care about (typos).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Regex for a diagnostic-code variant reference in source.
///
/// We match on `::[EWN]\d{4}` so the match requires a Rust path-access
/// operator immediately preceding the code.  This picks up both
/// `DiagCode::E1001` and `Code::W6003` (the two variant paths in the
/// workspace) while ignoring range literals in doc comments
/// (`E0001..=E0999`) and prose mentions (`the E4000 range`).
///
/// ASCII-only char classes so the regex crate stays slim (default
/// features off, unicode-perl excluded).
const CODE_REGEX: &str = r"(?-u)::([EWN][0-9]{4})\b";

/// Regex for a registry entry in `codes.rs`: a line of the form
/// `    E0001 = 1,` (four-space indent, variant name, `=`, discriminant).
const REGISTRY_REGEX: &str = r"(?m)(?-u)^ {4}([EWN][0-9]{4})[ \t]*=[ \t]*[0-9]+[ \t]*,";

/// Entry point for `cargo xtask check-diag-codes`.  Returns `Ok(())` iff
/// every diagnostic-code reference in `crates/**/*.rs` is registered in
/// `cypher-diag/src/codes.rs`.  Unused registry entries are printed as
/// info but do not fail the gate.
pub fn run() -> Result<()> {
    let root = workspace_root()?;
    let registered = load_registry(&root)?;
    let referenced = scan_references(&root)?;

    let orphans: BTreeSet<&String> = referenced.difference(&registered).collect();
    let unused: BTreeSet<&String> = registered.difference(&referenced).collect();

    println!(
        "check-diag-codes: registered={} referenced={} orphans={} unused={}",
        registered.len(),
        referenced.len(),
        orphans.len(),
        unused.len(),
    );

    if !unused.is_empty() {
        // Info-only: reserved codes are allowed.  Print so a reviewer can
        // audit the list during release prep.
        println!("unused registry entries (may be intentional reservations):");
        for code in &unused {
            println!("  {code}");
        }
    }

    if !orphans.is_empty() {
        eprintln!("orphan references — code used in source but not registered in codes.rs:");
        for code in &orphans {
            eprintln!("  {code}");
        }
        bail!("{} orphan diagnostic code(s) found", orphans.len());
    }

    println!("check-diag-codes: OK");
    Ok(())
}

fn load_registry(root: &Path) -> Result<BTreeSet<String>> {
    let path = root.join("crates/cypher-diag/src/codes.rs");
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let re = regex::Regex::new(REGISTRY_REGEX).expect("REGISTRY_REGEX must compile");
    let set: BTreeSet<String> = re
        .captures_iter(&content)
        .map(|c| c[1].to_owned())
        .collect();
    if set.is_empty() {
        bail!(
            "no registry entries parsed from {} — REGISTRY_REGEX out of sync with codes.rs?",
            path.display()
        );
    }
    Ok(set)
}

fn scan_references(root: &Path) -> Result<BTreeSet<String>> {
    let re = regex::Regex::new(CODE_REGEX).expect("CODE_REGEX must compile");
    let mut refs = BTreeSet::new();
    walk_rs_files(&root.join("crates"), &mut |path| {
        // Skip codes.rs itself — it declares variants, not references.
        if path.ends_with("crates/cypher-diag/src/codes.rs") {
            return Ok(());
        }
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        for cap in re.captures_iter(&content) {
            refs.insert(cap[1].to_owned());
        }
        Ok(())
    })?;
    Ok(refs)
}

/// Recursively walk `.rs` files under `root`, invoking `f` on each.
/// `target/` is skipped.  Errors propagate.
fn walk_rs_files(root: &Path, f: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    let entries = std::fs::read_dir(root).with_context(|| format!("reading {}", root.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("entry in {}", root.display()))?;
        let path = entry.path();
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_dir() {
            if file_name == "target" || file_name == "node_modules" || file_name.starts_with('.') {
                continue;
            }
            walk_rs_files(&path, f)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            f(&path)?;
        }
    }
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let here = std::env::current_dir().context("reading current dir")?;
    if here.join("Cargo.toml").exists() && here.join("crates").is_dir() {
        Ok(here)
    } else {
        bail!(
            "xtask check-diag-codes must be run from the workspace root \
             (expected Cargo.toml + crates/ at {})",
            here.display()
        )
    }
}
