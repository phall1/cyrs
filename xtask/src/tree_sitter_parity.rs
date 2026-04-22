//! `cargo xtask tree-sitter-parity` — interop gate for the
//! `tree-sitter-cypher` grammar (bead cy-od5.1).
//!
//! The Rust parser in `cypher-syntax` is authoritative, but we maintain a
//! parallel tree-sitter grammar in `tree-sitter-cypher/` for editor
//! integrations (Neovim, Helix, GitHub). This task keeps the two in
//! lock-step by running every TCK v1 scenario through both parsers and
//! comparing the shape of the result:
//!
//! - `outcome = "ok"`    → tree-sitter produces no `(ERROR)` node
//! - `outcome = "error"` → tree-sitter produces at least one `(ERROR)` node
//!
//! Mismatches are summarised at the end; any mismatch fails the task.
//!
//! No new Rust deps: the TOML reader is a small hand-rolled scanner that
//! understands the TCK v1 subset — scalar-only `[[scenario]]` arrays with
//! single-line string / array values.

#![allow(clippy::uninlined_format_args)]

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

/// Entry point wired from `xtask::main`.
pub fn run() -> Result<()> {
    let workspace = workspace_root();
    let grammar_dir = workspace.join("tree-sitter-cypher");
    let tck_file = workspace.join("crates/cypher-tck/tck/v1.toml");

    if !grammar_dir.is_dir() {
        bail!("tree-sitter-cypher/ not found at {}", grammar_dir.display());
    }
    if !tck_file.is_file() {
        bail!("TCK fixture not found at {}", tck_file.display());
    }

    let ts = resolve_tree_sitter(&grammar_dir).ok_or_else(|| {
        anyhow!(
            "`tree-sitter` CLI not found.\n\
             Install globally with `npm install -g tree-sitter-cli@0.24`\n\
             or run `npm install` inside tree-sitter-cypher/ and it will\n\
             be available as `node_modules/.bin/tree-sitter`."
        )
    })?;

    println!("==> tree-sitter: {}", ts.display());

    // 1. Generate parser + run grammar self-tests.
    println!("==> (cd tree-sitter-cypher && tree-sitter generate)");
    run_in(&grammar_dir, &ts, &["generate"])?;
    println!("==> (cd tree-sitter-cypher && tree-sitter test)");
    run_in(&grammar_dir, &ts, &["test"])?;

    // 2. Load scenarios and diff each one through tree-sitter parse.
    let toml_src = std::fs::read_to_string(&tck_file)
        .with_context(|| format!("reading {}", tck_file.display()))?;
    let mut scenarios =
        parse_scenarios(&toml_src).with_context(|| format!("parsing {}", tck_file.display()))?;

    // cy-h0p: supplementary scenarios for v1-grammar surfaces that aren't
    // yet in `v1.toml`. These exercise constructs the tree-sitter grammar
    // v1 accepts (UNION, list comp with `|`, list slice) and constructs
    // the spec §9.3 bans (block-form CALL / EXISTS subqueries, SHOW,
    // CYPHER prefix, LOAD CSV) to lock the rejection contract.
    scenarios.extend(supplementary_scenarios());

    println!(
        "==> loaded {} scenarios ({} from {} + supplementary v1-grammar cases)",
        scenarios.len(),
        scenarios.len() - SUPPLEMENTARY_COUNT,
        tck_file.display()
    );

    let mut mismatches: Vec<String> = Vec::new();
    let mut agreed = 0usize;
    for (idx, s) in scenarios.iter().enumerate() {
        let sexpr = parse_with_tree_sitter(&grammar_dir, &ts, &s.query)
            .with_context(|| format!("scenario #{idx} `{}`", s.name))?;
        let has_error = sexpr.contains("(ERROR") || sexpr.contains("MISSING");
        let expected_error = s.outcome == "error";
        if has_error == expected_error {
            agreed += 1;
        } else {
            mismatches.push(format!(
                "  - `{}` [outcome={}]: tree-sitter produced {} (ERROR)\n      query: {}",
                s.name,
                s.outcome,
                if has_error { "an" } else { "no" },
                s.query,
            ));
        }
    }

    println!(
        "==> parity: {}/{} scenarios agree with cyrs TCK v1 + supplement",
        agreed,
        scenarios.len()
    );

    if !mismatches.is_empty() {
        println!("\nmismatches:");
        for m in &mismatches {
            println!("{m}");
        }
        bail!(
            "{} / {} scenarios disagree between tree-sitter-cypher and cyrs TCK v1",
            mismatches.len(),
            scenarios.len()
        );
    }

    println!("==> tree-sitter-parity OK");
    Ok(())
}

/// Minimal scenario record. Matches the v1.toml shape we need.
#[derive(Debug)]
struct Scenario {
    name: String,
    query: String,
    outcome: String,
}

/// Number of supplementary scenarios appended by [`supplementary_scenarios`].
/// Kept as a compile-time constant so the harness's load-line can report the
/// TCK-sourced scenario count independently from the supplement.
const SUPPLEMENTARY_COUNT: usize = 12;

/// Grammar-v1 scenarios that aren't yet in `crates/cypher-tck/tck/v1.toml`
/// (this bead lives entirely inside `tree-sitter-cypher/` and cannot edit
/// the TCK fixture). Each entry locks an expected parse outcome for a
/// construct the tree-sitter grammar added in cy-h0p.
///
/// The `ok` entries cover surfaces the grammar accepts cleanly; the
/// `error` entries pin §9.3 bans so a future regression that lets them
/// parse without `(ERROR)` nodes would fail the harness.
fn supplementary_scenarios() -> Vec<Scenario> {
    fn sc(name: &str, query: &str, outcome: &str) -> Scenario {
        Scenario {
            name: name.to_string(),
            query: query.to_string(),
            outcome: outcome.to_string(),
        }
    }
    vec![
        // --- accepted v1 surfaces ---------------------------------------
        sc("UNION simple", "RETURN 1 UNION RETURN 2", "ok"),
        sc("UNION ALL", "RETURN 1 UNION ALL RETURN 2", "ok"),
        sc(
            "UNION across MATCH clauses",
            "MATCH (n) RETURN n UNION MATCH (m) RETURN m",
            "ok",
        ),
        sc(
            "List comprehension with predicate and map",
            "RETURN [x IN [1, 2, 3] WHERE x > 1 | x + 1]",
            "ok",
        ),
        sc(
            "List slice with both bounds",
            "RETURN [1, 2, 3][0..2]",
            "ok",
        ),
        sc("List slice elided upper", "RETURN [1, 2, 3][1..]", "ok"),
        sc("List slice elided lower", "RETURN [1, 2, 3][..2]", "ok"),
        // --- §9.3 bans --------------------------------------------------
        sc(
            "§9.3: CALL block subquery rejected",
            "CALL { MATCH (n) RETURN n } RETURN n",
            "error",
        ),
        sc(
            "§9.3: EXISTS block subquery rejected",
            "MATCH (n) WHERE EXISTS { MATCH (n)-->(m) } RETURN n",
            "error",
        ),
        sc("§9.3: SHOW is not a clause", "SHOW DATABASES", "error"),
        sc(
            "§9.3: CYPHER prefix rejected",
            "CYPHER runtime=slotted MATCH (n) RETURN n",
            "error",
        ),
        sc(
            "§9.3: LOAD CSV rejected",
            "LOAD CSV FROM 'file:///data.csv' AS row RETURN row",
            "error",
        ),
    ]
}

/// Parse the TCK v1 scenario TOML. Scalar-only, one-line values.
///
/// Grammar recognised (hand-written for this task — no new deps):
///   - `#` line comments
///   - `[[scenario]]` — begins a new scenario
///   - `name    = "..."`
///   - `query   = "..."`
///   - `outcome = "..."`
///   - other keys (`tags`, `note`) are silently ignored
fn parse_scenarios(src: &str) -> Result<Vec<Scenario>> {
    let mut out = Vec::new();
    let mut cur: Option<(Option<String>, Option<String>, Option<String>)> = None;

    for (lineno, raw) in src.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[scenario]]" {
            if let Some(done) = cur.take() {
                out.push(finish_scenario(done, lineno)?);
            }
            cur = Some((None, None, None));
            continue;
        }
        let Some((n, q, o)) = cur.as_mut() else {
            // Ignore any header above the first scenario.
            continue;
        };
        if let Some((key, val)) = split_kv(line) {
            match key {
                "name" => *n = Some(unquote(val, lineno)?),
                "query" => *q = Some(unquote(val, lineno)?),
                "outcome" => *o = Some(unquote(val, lineno)?),
                _ => {} // tags, note, etc. — not needed for parity
            }
        }
    }
    if let Some(done) = cur.take() {
        out.push(finish_scenario(done, usize::MAX)?);
    }
    Ok(out)
}

fn finish_scenario(
    (name, query, outcome): (Option<String>, Option<String>, Option<String>),
    lineno: usize,
) -> Result<Scenario> {
    Ok(Scenario {
        name: name.ok_or_else(|| anyhow!("scenario near line {lineno}: missing name"))?,
        query: query.ok_or_else(|| anyhow!("scenario near line {lineno}: missing query"))?,
        outcome: outcome.ok_or_else(|| anyhow!("scenario near line {lineno}: missing outcome"))?,
    })
}

/// Strip a `#` comment from a line, honouring `"` and `'` quoted strings
/// so `# inside a quoted string` is preserved.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_dq = false;
    let mut in_sq = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' if !in_sq => in_dq = !in_dq,
            b'\'' if !in_dq => in_sq = !in_sq,
            b'#' if !in_dq && !in_sq => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

fn split_kv(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    let val = line[eq + 1..].trim();
    if key.is_empty() || val.is_empty() {
        return None;
    }
    Some((key, val))
}

/// Very small string un-quoter covering the TCK fixture: "..." with `\'`,
/// `\"`, `\\`, `\n`, `\t` escapes.
fn unquote(raw: &str, lineno: usize) -> Result<String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        bail!("line {lineno}: expected double-quoted string, got `{raw}`");
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    bail!("line {lineno}: unknown escape `\\{other}` in {raw}");
                }
                None => bail!("line {lineno}: trailing backslash in {raw}"),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Resolve the `tree-sitter` binary: first the grammar-local
/// `node_modules/.bin/tree-sitter`, then whatever is on PATH.
fn resolve_tree_sitter(grammar_dir: &Path) -> Option<PathBuf> {
    let local = grammar_dir
        .join("node_modules")
        .join(".bin")
        .join("tree-sitter");
    if local.is_file() {
        return Some(local);
    }
    // Fall back to PATH — treat `--version` success as "present".
    match Command::new("tree-sitter")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(s) if s.success() => Some(PathBuf::from("tree-sitter")),
        _ => None,
    }
}

/// Run `tree-sitter <args>` in the grammar directory, inheriting stdio.
fn run_in(dir: &Path, ts: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new(ts)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                anyhow!("`{}` not found: {err}", ts.display())
            } else {
                anyhow!("failed to spawn `{}`: {err}", ts.display())
            }
        })?;
    if !status.success() {
        bail!(
            "`{} {}` in {} exited with {}",
            ts.display(),
            args.join(" "),
            dir.display(),
            status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        );
    }
    Ok(())
}

/// Pipe `query` into `tree-sitter parse --quiet` via stdin and capture the
/// s-expression output. Non-zero exit is not treated as a hard failure
/// here — tree-sitter returns exit code 1 on parse errors, which is the
/// state we want to detect by looking for `(ERROR)` in stdout.
fn parse_with_tree_sitter(grammar_dir: &Path, ts: &Path, query: &str) -> Result<String> {
    use std::io::Write;

    let mut child = Command::new(ts)
        .args(["parse", "--quiet", "/dev/stdin"])
        .current_dir(grammar_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning `{} parse`", ts.display()))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open tree-sitter stdin"))?;
        stdin.write_all(query.as_bytes())?;
        // Ensure trailing newline — tree-sitter's line-based error reporting
        // is more predictable with one.
        if !query.ends_with('\n') {
            stdin.write_all(b"\n")?;
        }
    }

    let output = child
        .wait_with_output()
        .context("waiting for tree-sitter parse")?;
    // Combine stdout and stderr — parse errors sometimes land on stderr as
    // `(ERROR ...)` annotations too.
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push('\n');
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(combined)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate has a parent directory")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tck_toml_subset() {
        let src = r#"
# a comment
[[scenario]]
name    = "foo"
tags    = ["@FOO"]
query   = "RETURN 1"
outcome = "ok"

[[scenario]]
name    = "bar"
query   = "RETURN"
outcome = "error"
"#;
        let out = parse_scenarios(src).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "foo");
        assert_eq!(out[0].query, "RETURN 1");
        assert_eq!(out[0].outcome, "ok");
        assert_eq!(out[1].outcome, "error");
    }

    #[test]
    fn preserves_hash_inside_quoted_string() {
        let s = strip_comment(r#"query = "RETURN '# not a comment'""#);
        assert_eq!(s, r#"query = "RETURN '# not a comment'""#);
    }
}
