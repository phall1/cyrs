//! `cargo xtask check-recovery-budget` — recovery-range coverage gate
//! (bead cy-gkh, spec 0001 §10.2 + §17.3).
//!
//! The recovery-property test at
//! `crates/cypher-syntax/tests/recovery_properties.rs` is the quality gate
//! for parser error recovery in the `E0000..=E0999` range (spec §10.2):
//! every recovery code must be exercised either directly by the property
//! test (which asserts bounded diagnostics + locality for random prefixes
//! of curated TCK sources) or indirectly by a compiletest UI fixture under
//! `crates/cypher-syntax/tests/ui/syntax/`.
//!
//! This gate flags any `E0xxx` code that is registered in the
//! [`cypher-diag` registry][codes] **and** referenced from parser emit
//! sites, but does not appear in either the property test file or a UI
//! fixture `.stderr`. The intent is to catch a PR that adds a recovery
//! code without also adding the fixture or property-test input that
//! exercises it — which would otherwise leave the code silently unexercised
//! and drift out of the property-test's curated corpus.
//!
//! [codes]: ../../../crates/cypher-diag/src/codes.rs
//!
//! # Scope
//!
//! - Only codes in the syntax range (`E0001..=E0999`) participate. Name
//!   resolution, semantic, dialect, type-system, warning, and note codes
//!   all have their own gates.
//! - Codes that are **declared but never emitted** (the `#[allow(dead_code)]`
//!   reservations that pre-stage a follow-up bead — e.g. `E0062` /
//!   `E0063` at the time of writing) are ignored: a reservation without
//!   an emit site cannot be exercised by definition, and the
//!   `check-diag-codes` gate already lists them as "unused registry
//!   entries".
//!
//! # Grep, not parse
//!
//! The gate uses a simple regex scan — the intent here is drift detection,
//! not correctness. A false positive is a fixture rename or a property
//! test refactor; a false negative is a missing fixture, and catches a
//! real bug. Both are resolved by a human looking at the diff.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Syntax-range lower bound (inclusive).
const SYNTAX_MIN: u16 = 1;
/// Syntax-range upper bound (inclusive).
const SYNTAX_MAX: u16 = 999;

/// Parser emit sites — these files reference recovery codes via
/// `sc::EXPECTED_…` constants. The constants resolve to `u16` values in
/// `crates/cypher-syntax/src/parser.rs`. This scan collects the *numeric*
/// codes emitted by the parser; anything registered in the `DiagCode`
/// enum but never emitted here is not in scope.
const PARSER_EMIT_SITES: &[&str] = &[
    "crates/cypher-syntax/src/grammar/clause.rs",
    "crates/cypher-syntax/src/grammar/expression.rs",
    "crates/cypher-syntax/src/grammar/pattern.rs",
    "crates/cypher-syntax/src/grammar/statement.rs",
    "crates/cypher-syntax/src/grammar/mod.rs",
    "crates/cypher-syntax/src/parser.rs",
];

/// Recovery-property test file (bead cy-gkh / §17.3). Codes referenced
/// here count as exercised by the property gate regardless of whether a
/// UI fixture exists.
const RECOVERY_PROPERTY_TEST: &str = "crates/cypher-syntax/tests/recovery_properties.rs";

/// UI fixture root. Every `.stderr` sidecar under this directory is
/// scanned for `E00xx` mentions; a code whose stderr contains `E00xx`
/// counts as exercised by the compiletest harness.
const UI_SYNTAX_DIR: &str = "crates/cypher-syntax/tests/ui/syntax";

/// Entry point for `cargo xtask check-recovery-budget`.
///
/// Returns `Ok(())` iff every parser-emitted syntax code in
/// `E0001..=E0999` is referenced by the recovery property test or a UI
/// `.stderr` fixture.
///
/// # Errors
///
/// Returns an error if the workspace root cannot be located, any file
/// cannot be read, or any code is unexercised.
pub fn run() -> Result<()> {
    let root = workspace_root()?;

    let emitted = scan_parser_emit_sites(&root)?;
    let property = scan_file_for_codes(&root.join(RECOVERY_PROPERTY_TEST))?;
    let ui = scan_ui_stderr_codes(&root.join(UI_SYNTAX_DIR))?;

    let exercised: BTreeSet<u16> = property.union(&ui).copied().collect();

    let missing: Vec<u16> = emitted
        .iter()
        .copied()
        .filter(|c| !exercised.contains(c))
        .collect();

    println!(
        "check-recovery-budget: emitted={} exercised={} missing={}",
        emitted.len(),
        exercised.len(),
        missing.len(),
    );

    if missing.is_empty() {
        println!("check-recovery-budget: OK");
        return Ok(());
    }

    eprintln!(
        "check-recovery-budget: FAIL — recovery codes have no UI fixture \
         and are not referenced by {RECOVERY_PROPERTY_TEST}:"
    );
    for code in &missing {
        eprintln!("  E{code:04}");
    }
    eprintln!(
        "\nHINT: add a `.cypher` + `.stderr` fixture under {UI_SYNTAX_DIR}/ \
         or extend the SOURCES list in {RECOVERY_PROPERTY_TEST}."
    );
    bail!("{} unexercised recovery code(s)", missing.len())
}

// ---------------------------------------------------------------------------
// Scanners
// ---------------------------------------------------------------------------

/// Collect every numeric code emitted by the parser. Matches lines of the
/// form `sc::EXPECTED_…` / `sc::UNCLOSED_…` / `sc::INVALID_…` /
/// `sc::UNIMPLEMENTED_…` — the full set of recovery-code constants
/// declared at the top of `parser.rs`. Resolves the constant's numeric
/// value by grepping `parser.rs` for its declaration.
fn scan_parser_emit_sites(root: &Path) -> Result<BTreeSet<u16>> {
    // First pass: build a name → numeric map from the constants file.
    let parser_rs = root.join("crates/cypher-syntax/src/parser.rs");
    let parser_src = std::fs::read_to_string(&parser_rs)
        .with_context(|| format!("reading {}", parser_rs.display()))?;
    // ASCII-only regex classes so the regex crate stays slim (default
    // features + perf only, no unicode-perl). `[ \t]*` covers horizontal
    // whitespace (tabs + spaces); declaration lines never wrap.
    let decl_re = regex::Regex::new(
        r"(?m)(?-u)^[ \t]*pub\(crate\)[ \t]+const[ \t]+([A-Z][A-Z0-9_]+)[ \t]*:[ \t]*u16[ \t]*=[ \t]*([0-9]+)[ \t]*;",
    )
    .expect("declaration regex must compile");
    let mut name_to_code: std::collections::BTreeMap<String, u16> =
        std::collections::BTreeMap::new();
    for cap in decl_re.captures_iter(&parser_src) {
        let name = cap[1].to_string();
        let code: u16 = cap[2].parse().context("numeric constant")?;
        if (SYNTAX_MIN..=SYNTAX_MAX).contains(&code) {
            name_to_code.insert(name, code);
        }
    }

    // Second pass: find each `sc::<NAME>` usage across the emit sites.
    let use_re =
        regex::Regex::new(r"(?-u)\bsc::([A-Z][A-Z0-9_]+)\b").expect("use regex must compile");
    let mut emitted: BTreeSet<u16> = BTreeSet::new();
    for rel in PARSER_EMIT_SITES {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        for cap in use_re.captures_iter(&src) {
            if let Some(code) = name_to_code.get(&cap[1]) {
                emitted.insert(*code);
            }
        }
    }
    Ok(emitted)
}

/// Grep a file for `E00xx` mentions in the syntax range.
fn scan_file_for_codes(path: &Path) -> Result<BTreeSet<u16>> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(collect_codes_from_str(&src))
}

/// Walk `dir` recursively and collect every code referenced in any
/// `.stderr` sidecar.
fn scan_ui_stderr_codes(dir: &Path) -> Result<BTreeSet<u16>> {
    let mut out = BTreeSet::new();
    if !dir.exists() {
        return Ok(out);
    }
    walk_stderr_files(dir, &mut |path| {
        let src =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        out.extend(collect_codes_from_str(&src));
        Ok(())
    })?;
    Ok(out)
}

fn walk_stderr_files(dir: &Path, f: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            walk_stderr_files(&p, f)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("stderr") {
            f(&p)?;
        }
    }
    Ok(())
}

fn collect_codes_from_str(src: &str) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();

    // First: expand inclusive range forms like `E0012..=E0017` into every
    // code in the span. Ranges let the registry comment block in
    // `tests/recovery_properties.rs` stay compact without a per-code
    // bullet for every reservation.
    let range_re = regex::Regex::new(r"(?-u)\bE([0-9]{4})\.\.=?E([0-9]{4})\b")
        .expect("range regex must compile");
    for cap in range_re.captures_iter(src) {
        let lo: u16 = cap[1].parse().unwrap_or(0);
        let hi: u16 = cap[2].parse().unwrap_or(0);
        if lo == 0 || hi < lo {
            continue;
        }
        for n in lo..=hi {
            if (SYNTAX_MIN..=SYNTAX_MAX).contains(&n) {
                out.insert(n);
            }
        }
    }

    // Then: individual codes. The range regex above also matches the two
    // anchors, which are fine — the set is idempotent.
    let re = regex::Regex::new(r"(?-u)\bE([0-9]{4})\b").expect("code regex must compile");
    for cap in re.captures_iter(src) {
        if let Ok(n) = cap[1].parse::<u16>()
            && (SYNTAX_MIN..=SYNTAX_MAX).contains(&n)
        {
            out.insert(n);
        }
    }
    out
}

fn workspace_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR of xtask = <workspace>/xtask. Parent is root. This
    // works both under `cargo run -p xtask` (cwd = workspace root) and under
    // `cargo test -p xtask` (cwd = xtask/) — mirror of `check_recovery`.
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask crate has a parent directory")?;
    if base.join("Cargo.toml").exists() && base.join("crates").is_dir() {
        Ok(base)
    } else {
        bail!(
            "xtask check-recovery-budget: workspace not found at {}",
            base.display()
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_codes_grabs_syntax_range_only() {
        let s = "error[E0047]: … E1001 also W6001";
        let got = collect_codes_from_str(s);
        assert!(got.contains(&47));
        assert!(!got.contains(&1001));
        // W6001 is not in the E-regex; absent by construction.
    }

    #[test]
    fn collect_codes_expands_inclusive_ranges() {
        let s = "// Exercised: E0012..=E0017 — rel / path recovery.";
        let got = collect_codes_from_str(s);
        for n in 12..=17 {
            assert!(got.contains(&n), "missing E{n:04}");
        }
    }

    /// Smoke test: the checked-in parser emits ⊆ exercised-set on main.
    /// If this fails, run `cargo xtask check-recovery-budget` locally —
    /// the test body and the gate share implementation.
    #[test]
    fn workspace_passes_recovery_budget() {
        run().expect("every parser-emitted recovery code must be exercised");
    }
}
