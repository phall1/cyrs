//! `cargo xtask check-recovery` — spec §4.3 / §17.18 coverage invariant.
//!
//! Parses the two normative artefacts — `crates/cyrs-ast/cypher.ungrammar`
//! and `crates/cyrs-syntax/docs/recovery.md` — into their production-name
//! sets and fails with a two-sided diff on any asymmetry. The invariant is:
//! every production named in the ungrammar has an entry in `recovery.md`,
//! and every entry in `recovery.md` names a live production.
//!
//! Runs as a blocking PR gate per spec §17.18. Not part of the local
//! pre-commit `gate` — that gate targets fast feedback; recovery-table
//! drift surfaces on PR CI.
//!
//! The ungrammar preprocessing (the `+` → `X X*` and bare-SCREAMING_SNAKE
//! quoting steps) is duplicated from `xtask::codegen` on purpose — this
//! bead is file-disjoint from cy-dpq so we avoid touching `codegen.rs`.
//! TODO: dedupe with `codegen::desugar_one_or_more` /
//! `codegen::quote_screaming_snake_idents` once both beads have landed.

#![allow(clippy::uninlined_format_args)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::doc_markdown)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

/// Entry point called from `xtask::main`. Verifies the coverage invariant.
pub fn run() -> Result<()> {
    let workspace = workspace_root();
    let ungrammar = workspace.join("crates/cyrs-ast/cypher.ungrammar");
    let recovery = workspace.join("crates/cyrs-syntax/docs/recovery.md");

    let grammar_names = parse_ungrammar_file(&ungrammar)
        .with_context(|| format!("parsing {}", ungrammar.display()))?;
    let recovery_names = parse_recovery_file(&recovery)
        .with_context(|| format!("parsing {}", recovery.display()))?;

    report(&grammar_names, &recovery_names)
}

fn report(grammar_names: &BTreeSet<String>, recovery_names: &BTreeSet<String>) -> Result<()> {
    let missing_in_recovery: Vec<&str> = grammar_names
        .difference(recovery_names)
        .map(String::as_str)
        .collect();
    let extra_in_recovery: Vec<&str> = recovery_names
        .difference(grammar_names)
        .map(String::as_str)
        .collect();

    if missing_in_recovery.is_empty() && extra_in_recovery.is_empty() {
        println!("check-recovery: OK ({} productions)", grammar_names.len());
        return Ok(());
    }

    eprintln!("check-recovery: FAIL (spec §4.3 coverage invariant)\n");
    if !missing_in_recovery.is_empty() {
        eprintln!("productions missing from recovery.md:");
        for n in &missing_in_recovery {
            eprintln!("  - {}", n);
        }
        eprintln!();
    }
    if !extra_in_recovery.is_empty() {
        eprintln!("recovery.md entries for productions that no longer exist:");
        for n in &extra_in_recovery {
            eprintln!("  - {}", n);
        }
        eprintln!();
    }
    anyhow::bail!("grammar<->recovery.md asymmetry")
}

// =====================================================================
// Filesystem helpers
// =====================================================================

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR of xtask = <workspace>/xtask. Parent is root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate has a parent directory")
        .to_path_buf()
}

// =====================================================================
// Ungrammar parsing
// =====================================================================

fn parse_ungrammar_file(path: &Path) -> Result<BTreeSet<String>> {
    let src = std::fs::read_to_string(path)?;
    parse_ungrammar_str(&src)
}

fn parse_ungrammar_str(src: &str) -> Result<BTreeSet<String>> {
    let preprocessed = quote_screaming_snake_idents(&desugar_one_or_more(src));
    let grammar: ungrammar::Grammar = preprocessed
        .parse()
        .map_err(|e| anyhow!("ungrammar parse: {e}"))?;
    Ok(grammar.iter().map(|n| grammar[n].name.clone()).collect())
}

/// Rewrite unquoted `X+` into `X X*`. Preserves quoted strings and `//`
/// comments. Mirror of `xtask::codegen::desugar_one_or_more`; see module
/// docs for the TODO about deduplication.
fn desugar_one_or_more(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 32);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;

        if c == '\'' {
            out.push(c);
            i += 1;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                out.push(ch);
                i += 1;
                if ch == '\'' {
                    break;
                }
            }
            continue;
        }

        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
            while i < bytes.len() && bytes[i] as char != '\n' {
                out.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }

        if c == '+' {
            let prev = last_non_space(out.as_bytes());
            let is_rep = matches!(prev, Some(b')') | Some(b'*'))
                || prev.is_some_and(|b| (b as char).is_ascii_alphanumeric() || b == b'_');
            if is_rep {
                let token = extract_trailing_token(&out);
                if !token.is_empty() {
                    out.push(' ');
                    out.push_str(&token);
                    out.push('*');
                    i += 1;
                    continue;
                }
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

/// Wrap bare SCREAMING_SNAKE_CASE identifiers in single quotes. Mirror of
/// `xtask::codegen::quote_screaming_snake_idents`.
fn quote_screaming_snake_idents(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 32);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;

        if c == '\'' {
            out.push(c);
            i += 1;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                out.push(ch);
                i += 1;
                if ch == '\'' {
                    break;
                }
            }
            continue;
        }

        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
            while i < bytes.len() && bytes[i] as char != '\n' {
                out.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }

        if c.is_ascii_uppercase() || c == '_' {
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &src[start..i];
            let is_screaming = ident.len() > 1
                && ident
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
            if is_screaming {
                out.push('\'');
                out.push_str(ident);
                out.push('\'');
            } else {
                out.push_str(ident);
            }
            continue;
        }

        out.push(c);
        i += 1;
    }
    out
}

fn last_non_space(buf: &[u8]) -> Option<u8> {
    buf.iter()
        .rev()
        .copied()
        .find(|b| !(*b as char).is_whitespace())
}

/// Pull the trailing token (an identifier or balanced `(...)` group) off
/// the tail of `buf`. Returns a copy; `buf` is left untouched. Mirror of
/// `xtask::codegen::extract_trailing_token`.
fn extract_trailing_token(buf: &str) -> String {
    let bytes = buf.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] as char).is_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return String::new();
    }

    match bytes[end - 1] as char {
        ')' => {
            let mut depth = 1i32;
            let mut i = end - 1;
            while i > 0 {
                i -= 1;
                match bytes[i] as char {
                    ')' => depth += 1,
                    '(' => {
                        depth -= 1;
                        if depth == 0 {
                            return std::str::from_utf8(&bytes[i..end])
                                .unwrap_or("")
                                .to_string();
                        }
                    }
                    _ => {}
                }
            }
            String::new()
        }
        c if c.is_ascii_alphanumeric() || c == '_' => {
            let mut start = end;
            while start > 0 {
                let b = bytes[start - 1] as char;
                if b.is_ascii_alphanumeric() || b == '_' {
                    start -= 1;
                } else {
                    break;
                }
            }
            std::str::from_utf8(&bytes[start..end])
                .unwrap_or("")
                .to_string()
        }
        _ => String::new(),
    }
}

// =====================================================================
// Recovery.md parsing
// =====================================================================

fn parse_recovery_file(path: &Path) -> Result<BTreeSet<String>> {
    let src = std::fs::read_to_string(path)?;
    Ok(parse_recovery_str(&src))
}

/// Collect every H3 heading whose text is an identifier-shaped, PascalCase
/// production name. Other H3 headings (e.g. section dividers) are ignored.
fn parse_recovery_str(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("### ") {
            let name = rest.trim();
            if looks_like_production(name) {
                out.insert(name.to_string());
            }
        }
    }
    out
}

fn looks_like_production(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recovery_extracts_h3_production_headings() {
        let md = "\
# Title

## Section

### MatchClause

body

### OrderBy

body

## other
";
        let names = parse_recovery_str(md);
        assert!(names.contains("MatchClause"));
        assert!(names.contains("OrderBy"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn parse_recovery_ignores_non_production_h3_text() {
        let md = "\
### Section: Foo

### validProductionName

### Another heading with spaces
";
        let names = parse_recovery_str(md);
        // All three should be rejected: `Section: Foo` has `:`, the second
        // starts lowercase, the third has spaces.
        assert!(names.is_empty(), "got: {names:?}");
    }

    #[test]
    fn parse_ungrammar_extracts_production_names() {
        let src = "\
Foo = 'a' Bar+
Bar = 'b' | 'c'
";
        let names = parse_ungrammar_str(src).expect("parse");
        assert!(names.contains("Foo"));
        assert!(names.contains("Bar"));
        assert_eq!(names.len(), 2);
    }

    /// Smoke test: the checked-in recovery.md is in sync with the
    /// checked-in ungrammar. Also acts as the drift detector — if the
    /// grammar or the table shifts without the other being updated,
    /// `cargo test -p xtask` fails.
    #[test]
    fn workspace_files_pass_check_recovery() {
        run().expect("recovery.md is in sync with cypher.ungrammar");
    }
}
