//! `cargo xtask gql-rules` — extract a deterministic rule manifest from
//! the vendored opengql `GQL.g4` ANTLR4 grammar (bead cy-7hn0).
//!
//! The xtask is intentionally JVM-free: we never invoke `antlr4`, we never
//! depend on `antlr-rust`. ANTLR4 `.g4` syntax is regular enough for our
//! needs (we ignore embedded actions, semantic predicates, and lexer
//! modes — none of which appear in `GQL.g4` beyond per-rule `options
//! { caseInsensitive = false; }` blocks which we strip).
//!
//! Output:
//!   * `crates/cyrs-tck/tck/opengql-grammar/rules.json` — machine readable
//!   * `crates/cyrs-tck/tck/opengql-grammar/rules.md`   — human readable
//!
//! Both are written deterministically; running the xtask twice in a row
//! must produce a zero diff. Downstream beads consume `rules.json` to
//! drive coverage tracking; this initial pass leaves every
//! `Implemented?` checkbox empty.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

/// One entry in the rule manifest.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Rule identifier as declared in the `.g4` source.
    pub name: String,
    /// Parser / lexer / fragment classification.
    pub kind: RuleKind,
    /// 1-based inclusive line where the rule header begins.
    pub line_start: usize,
    /// 1-based inclusive line of the terminating `;`.
    pub line_end: usize,
    /// Count of top-level alternatives (split on `|` at paren-depth 0,
    /// ignoring `|` inside `[…]` character classes, `'…'` string
    /// literals, or `(…)` groups).
    pub alts: usize,
    /// Names referenced from the body (other rule names + token names).
    /// Sorted, deduplicated.
    pub refs: Vec<String>,
}

/// Parser / lexer / fragment classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// Parser rule — name begins with a lowercase letter.
    Parser,
    /// Lexer rule — name is `ALL_UPPERCASE`.
    Lexer,
    /// `fragment` lexer rule — never produces a token on its own.
    Fragment,
}

impl RuleKind {
    fn as_str(self) -> &'static str {
        match self {
            RuleKind::Parser => "parser",
            RuleKind::Lexer => "lexer",
            RuleKind::Fragment => "fragment",
        }
    }
}

/// Workspace-rooted location of the vendored grammar.
const G4_REL: &str = "crates/cyrs-tck/tck/opengql-grammar/GQL.g4";

/// Entry point invoked from `xtask::main`.
pub fn run() -> Result<()> {
    let workspace_root = workspace_root()?;
    let g4_path = workspace_root.join(G4_REL);
    let grammar = fs::read_to_string(&g4_path)
        .with_context(|| format!("reading vendored grammar at {}", g4_path.display()))?;

    let rules = parse_rules(&grammar)?;

    let json_path = workspace_root.join("crates/cyrs-tck/tck/opengql-grammar/rules.json");
    let md_path = workspace_root.join("crates/cyrs-tck/tck/opengql-grammar/rules.md");
    fs::write(&json_path, render_json(&rules))
        .with_context(|| format!("writing {}", json_path.display()))?;
    fs::write(&md_path, render_md(&rules))
        .with_context(|| format!("writing {}", md_path.display()))?;

    let (parser_n, lexer_n, fragment_n) = counts(&rules);
    println!(
        "[xtask gql-rules] total={} parser={} lexer={} fragment={}",
        rules.len(),
        parser_n,
        lexer_n,
        fragment_n,
    );
    println!("  wrote {}", json_path.display());
    println!("  wrote {}", md_path.display());
    Ok(())
}

fn counts(rules: &[Rule]) -> (usize, usize, usize) {
    let mut p = 0;
    let mut l = 0;
    let mut f = 0;
    for r in rules {
        match r.kind {
            RuleKind::Parser => p += 1,
            RuleKind::Lexer => l += 1,
            RuleKind::Fragment => f += 1,
        }
    }
    (p, l, f)
}

/// Locate the workspace root by walking up from `CARGO_MANIFEST_DIR`
/// until a `Cargo.toml` with a `[workspace]` table is found.
fn workspace_root() -> Result<PathBuf> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in here.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if candidate.is_file() {
            let body = fs::read_to_string(&candidate).unwrap_or_default();
            if body.contains("[workspace]") {
                return Ok(ancestor.to_path_buf());
            }
        }
    }
    Err(anyhow!(
        "could not locate workspace Cargo.toml above {}",
        here.display()
    ))
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Sentinel returned by [`Scanner::next_significant`] when we have eaten
/// a comment or whitespace.
#[derive(Debug, Clone, Copy)]
enum Skip {
    None,
    Whitespace,
    Comment,
}

/// Tiny scanner over the grammar source. Tracks 1-based line numbers
/// and exposes byte offsets so we can slice rule bodies.
struct Scanner<'src> {
    src: &'src [u8],
    pos: usize,
    line: usize,
}

impl<'src> Scanner<'src> {
    fn new(src: &'src str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
        }
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn bump(&mut self) {
        if let Some(b) = self.peek() {
            if b == b'\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
    }

    /// Skip exactly one whitespace char or one comment. Returns
    /// `Skip::None` if nothing was eaten.
    fn skip_once(&mut self) -> Skip {
        match (self.peek(), self.peek2()) {
            (Some(c), _) if c.is_ascii_whitespace() => {
                self.bump();
                Skip::Whitespace
            }
            (Some(b'/'), Some(b'/')) => {
                // Line comment
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        break;
                    }
                    self.bump();
                }
                Skip::Comment
            }
            (Some(b'/'), Some(b'*')) => {
                self.bump();
                self.bump();
                while let (Some(a), Some(b)) = (self.peek(), self.peek2()) {
                    if a == b'*' && b == b'/' {
                        self.bump();
                        self.bump();
                        break;
                    }
                    self.bump();
                }
                Skip::Comment
            }
            _ => Skip::None,
        }
    }

    fn skip_trivia(&mut self) {
        while !self.eof() {
            if matches!(self.skip_once(), Skip::None) {
                break;
            }
        }
    }

    /// At the start of a candidate rule header. Try to consume:
    ///
    ///   - optional `fragment` keyword
    ///   - identifier
    ///
    /// Returns `(name, is_fragment, line_start)` on success and leaves
    /// the cursor immediately after the identifier; otherwise returns
    /// `None` and the cursor is unchanged.
    fn try_rule_header(&mut self) -> Option<(String, bool, usize)> {
        let line_start = self.line;
        let ident1 = self.consume_ident()?;
        if ident1 == "fragment" {
            self.skip_trivia();
            let ident2 = self.consume_ident()?;
            return Some((ident2, true, line_start));
        }
        Some((ident1, false, line_start))
    }

    /// Consume `[A-Za-z_][A-Za-z_0-9]*` from the current position.
    fn consume_ident(&mut self) -> Option<String> {
        let start = self.pos;
        let first = self.peek()?;
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return None;
        }
        self.bump();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.bump();
            } else {
                break;
            }
        }
        let bytes = &self.src[start..self.pos];
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    }
}

/// Extract every rule definition from a `.g4` source string.
pub fn parse_rules(src: &str) -> Result<Vec<Rule>> {
    let mut s = Scanner::new(src);
    let mut out = Vec::new();

    // Skip the `grammar X;` and top-level `options { … }` preamble by
    // letting the main loop walk through them — they are not rules
    // because their head is the *keyword* `grammar` / `options`, which
    // we filter out explicitly.
    loop {
        s.skip_trivia();
        if s.eof() {
            break;
        }

        // We're either at a candidate rule header, or some syntactic
        // construct we don't recognise (e.g. `grammar GQL;` preamble).
        // We attempt a header parse; on success we look for `:` (with
        // optional `options { … }` in between) and a body terminating
        // in `;`. On failure we bump one byte and continue scanning.

        let snapshot = (s.pos, s.line);
        let Some((name, is_fragment, line_start)) = s.try_rule_header() else {
            // Couldn't even read an identifier; bump and retry.
            s.bump();
            continue;
        };

        // Reject reserved prelude keywords that look like rule names.
        if matches!(name.as_str(), "grammar" | "import") {
            // Form: `grammar Foo;` / `import bar;`. Skip to the next `;`.
            skip_until_semicolon(&mut s);
            continue;
        }
        if matches!(name.as_str(), "options" | "tokens" | "channels") {
            // Form: `options { … }`. Skip the balanced brace block; no
            // trailing `;` in ANTLR4 grammar-level options.
            s.skip_trivia();
            if s.peek() == Some(b'{') {
                skip_balanced_braces(&mut s);
            }
            // Tolerate an optional trailing `;`.
            s.skip_trivia();
            if s.peek() == Some(b';') {
                s.bump();
            }
            continue;
        }

        // Skip whitespace + comments + optional `options { … }` block,
        // looking for `:`.
        s.skip_trivia();
        // Optional per-rule options block.
        if s.peek() == Some(b'o') && s.src[s.pos..].starts_with(b"options") {
            // Advance past `options` then skip `{ … }`.
            for _ in 0..b"options".len() {
                s.bump();
            }
            s.skip_trivia();
            if s.peek() == Some(b'{') {
                skip_balanced_braces(&mut s);
            }
            s.skip_trivia();
        }

        if s.peek() != Some(b':') {
            // Not actually a rule header — restore and bump past the
            // first byte to keep scanning.
            s.pos = snapshot.0;
            s.line = snapshot.1;
            s.bump();
            continue;
        }
        // Consume `:`.
        s.bump();

        // Capture body up to the next `;` at depth 0 (respecting
        // strings, char classes, and parens).
        let body_start = s.pos;
        let line_end = consume_rule_body(&mut s)?;
        let body_end = s.pos.saturating_sub(1); // exclude `;`
        let body = std::str::from_utf8(&s.src[body_start..body_end])
            .with_context(|| format!("rule {name} body not UTF-8"))?;

        let alts = count_alts(body);
        let refs = collect_refs(body);

        let kind = if is_fragment {
            RuleKind::Fragment
        } else if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            RuleKind::Lexer
        } else {
            RuleKind::Parser
        };

        out.push(Rule {
            name,
            kind,
            line_start,
            line_end,
            alts,
            refs,
        });
    }

    Ok(out)
}

/// Walk forward until we find a `;` at depth 0 outside of strings /
/// char classes / parens / braces / comments. Returns the 1-based line
/// of the `;`.  Cursor stops one past the `;`.
fn consume_rule_body(s: &mut Scanner<'_>) -> Result<usize> {
    let mut paren = 0i32;
    let mut brace = 0i32;
    loop {
        if s.eof() {
            return Err(anyhow!("unterminated rule body at line {}", s.line));
        }
        // Comments inside the body.
        match s.skip_once() {
            Skip::None => {}
            _ => continue,
        }
        let c = s.peek().unwrap();
        match c {
            b';' if paren == 0 && brace == 0 => {
                let line = s.line;
                s.bump();
                return Ok(line);
            }
            b'(' => {
                paren += 1;
                s.bump();
            }
            b')' => {
                paren -= 1;
                s.bump();
            }
            b'{' => {
                // Embedded action or semantic predicate. Skip balanced.
                brace += 1;
                s.bump();
                while brace > 0 && !s.eof() {
                    match s.peek().unwrap() {
                        b'{' => {
                            brace += 1;
                            s.bump();
                        }
                        b'}' => {
                            brace -= 1;
                            s.bump();
                        }
                        _ => s.bump(),
                    }
                }
            }
            b'\'' => {
                consume_quoted(s, b'\'');
            }
            b'"' => {
                consume_quoted(s, b'"');
            }
            b'[' => {
                consume_char_class(s);
            }
            _ => s.bump(),
        }
    }
}

fn consume_quoted(s: &mut Scanner<'_>, quote: u8) {
    s.bump(); // opening
    while let Some(c) = s.peek() {
        if c == b'\\' {
            s.bump();
            if s.peek().is_some() {
                s.bump();
            }
            continue;
        }
        if c == quote {
            s.bump();
            return;
        }
        s.bump();
    }
}

fn consume_char_class(s: &mut Scanner<'_>) {
    s.bump(); // `[`
    while let Some(c) = s.peek() {
        if c == b'\\' {
            s.bump();
            if s.peek().is_some() {
                s.bump();
            }
            continue;
        }
        if c == b']' {
            s.bump();
            return;
        }
        s.bump();
    }
}

fn skip_balanced_braces(s: &mut Scanner<'_>) {
    let mut depth = 0i32;
    while let Some(c) = s.peek() {
        match c {
            b'{' => {
                depth += 1;
                s.bump();
            }
            b'}' => {
                depth -= 1;
                s.bump();
                if depth == 0 {
                    return;
                }
            }
            b'\'' => consume_quoted(s, b'\''),
            b'"' => consume_quoted(s, b'"'),
            _ => s.bump(),
        }
    }
}

fn skip_until_semicolon(s: &mut Scanner<'_>) {
    while let Some(c) = s.peek() {
        match c {
            b';' => {
                s.bump();
                return;
            }
            b'\'' => consume_quoted(s, b'\''),
            b'"' => consume_quoted(s, b'"'),
            b'[' => consume_char_class(s),
            b'{' => skip_balanced_braces(s),
            b'/' if s.peek2() == Some(b'/') || s.peek2() == Some(b'*') => {
                s.skip_once();
            }
            _ => s.bump(),
        }
    }
}

/// Count top-level `|` alternatives in a rule body. We must respect
/// nesting (parens, char classes, strings, braces) so that
/// `(A | B)` inside a body counts as a *single* top-level alt.
fn count_alts(body: &str) -> usize {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut alts = 1usize;
    let mut paren = 0i32;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\'' | b'"' => {
                let quote = c;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'[' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b']' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'{' => {
                let mut depth = 1i32;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') => {
                if bytes[i + 1] == b'/' {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    i = (i + 2).min(bytes.len());
                }
            }
            b'(' => {
                paren += 1;
                i += 1;
            }
            b')' => {
                paren -= 1;
                i += 1;
            }
            b'|' if paren == 0 => {
                alts += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    alts
}

/// Collect every identifier referenced inside a rule body, ignoring
/// identifiers inside string literals, char classes, embedded actions,
/// and comments. Returns sorted + deduplicated.
fn collect_refs(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\'' | b'"' => {
                let q = c;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == q {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'[' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b']' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'{' => {
                let mut depth = 1i32;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') => {
                if bytes[i + 1] == b'/' {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    i = (i + 2).min(bytes.len());
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let ident = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
                // ANTLR keywords that can appear in bodies: EOF.
                // Strip ANTLR-meta tokens, and skip plain `EOF` because
                // it isn't a rule.
                if !ident.is_empty() && !is_meta_keyword(ident) {
                    seen.insert(ident.to_string());
                }
            }
            _ => i += 1,
        }
    }
    seen.into_iter().collect()
}

fn is_meta_keyword(s: &str) -> bool {
    matches!(
        s,
        "EOF"
            | "fragment"
            | "options"
            | "channel"
            | "skip"
            | "more"
            | "popMode"
            | "pushMode"
            | "mode"
            | "type"
            | "HIDDEN"
            | "DEFAULT_TOKEN_CHANNEL"
    )
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render `rules.json` with manual formatting so the output is stable
/// regardless of which serde version is in the lockfile. One-line-per-
/// rule layout keeps diffs reviewable.
fn render_json(rules: &[Rule]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    let total = rules.len();
    let _ = writeln!(out, "  \"total\": {total},");
    let (p, l, f) = counts(rules);
    let _ = writeln!(out, "  \"parser\": {p},");
    let _ = writeln!(out, "  \"lexer\": {l},");
    let _ = writeln!(out, "  \"fragment\": {f},");
    out.push_str("  \"rules\": [\n");
    for (i, r) in rules.iter().enumerate() {
        out.push_str("    {");
        let name = json_str(&r.name);
        let kind = json_str(r.kind.as_str());
        let line_start = r.line_start;
        let line_end = r.line_end;
        let alts = r.alts;
        let _ = write!(out, "\"name\": {name}, ");
        let _ = write!(out, "\"kind\": {kind}, ");
        let _ = write!(out, "\"line_start\": {line_start}, ");
        let _ = write!(out, "\"line_end\": {line_end}, ");
        let _ = write!(out, "\"alts\": {alts}, ");
        out.push_str("\"refs\": [");
        for (j, rf) in r.refs.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str(&json_str(rf));
        }
        out.push(']');
        out.push('}');
        if i + 1 < rules.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let code = c as u32;
                let _ = write!(out, "\\u{code:04x}");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_md(rules: &[Rule]) -> String {
    let mut out = String::new();
    out.push_str("# opengql `GQL.g4` rule manifest\n");
    out.push('\n');
    out.push_str(
        "Generated by `cargo xtask gql-rules` from the vendored grammar at\n\
         `crates/cyrs-tck/tck/opengql-grammar/GQL.g4`. Do not edit by hand —\n\
         regenerate after re-vendoring upstream. Bead: cy-7hn0.\n",
    );
    out.push('\n');
    let total = rules.len();
    let (p, l, f) = counts(rules);
    let _ = writeln!(
        out,
        "Totals: **{total} rules** ({p} parser, {l} lexer, {f} fragment)."
    );
    out.push('\n');
    out.push_str(
        "The `Implemented?` column is **unchecked across the board** in this\n\
         initial pass; downstream beads will tick boxes as the parser grows\n\
         coverage for each production.\n",
    );
    out.push('\n');
    out.push_str("| Rule | Kind | Line | Alts | Refs | Implemented? |\n");
    out.push_str("| --- | --- | ---: | ---: | --- | :---: |\n");
    for r in rules {
        let line_cell = if r.line_start == r.line_end {
            r.line_start.to_string()
        } else {
            format!("{}\u{2013}{}", r.line_start, r.line_end)
        };
        let refs_cell = if r.refs.is_empty() {
            String::from("\u{2014}")
        } else {
            r.refs
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let name = &r.name;
        let kind = r.kind.as_str();
        let alts = r.alts;
        let _ = writeln!(
            out,
            "| `{name}` | {kind} | {line_cell} | {alts} | {refs_cell} | [ ] |"
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_grammar() {
        let src = r"
grammar Toy;

options { caseInsensitive = true; }

// 1 <toy>
gqlProgram
    : a b? EOF
    | c
    ;

FOO : 'foo' ;
fragment BAR : [a-z]+ ;
";
        let rules = parse_rules(src).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].name, "gqlProgram");
        assert_eq!(rules[0].kind, RuleKind::Parser);
        assert_eq!(rules[0].alts, 2);
        assert_eq!(rules[0].refs, vec!["a", "b", "c"]);
        assert_eq!(rules[1].name, "FOO");
        assert_eq!(rules[1].kind, RuleKind::Lexer);
        assert_eq!(rules[1].alts, 1);
        assert!(rules[1].refs.is_empty());
        assert_eq!(rules[2].name, "BAR");
        assert_eq!(rules[2].kind, RuleKind::Fragment);
    }

    #[test]
    fn alts_respect_nesting() {
        // `(A | B) | C` is 2 top-level alts, not 3.
        let src = r"
grammar T;
r : (A | B) | C ;
A : 'a';
B : 'b';
C : 'c';
";
        let rules = parse_rules(src).unwrap();
        assert_eq!(rules[0].alts, 2);
    }

    #[test]
    fn handles_inline_options_after_rule_name() {
        let src = r"
grammar T;
fragment ESC options { caseInsensitive=false; }: '\\';
";
        let rules = parse_rules(src).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "ESC");
        assert_eq!(rules[0].kind, RuleKind::Fragment);
    }

    #[test]
    fn refs_ignore_strings_and_char_classes() {
        let src = r"
grammar T;
X : 'hello' [a-z]+ Y -> channel(HIDDEN) ;
Y : 'y' ;
";
        let rules = parse_rules(src).unwrap();
        // For X, only `Y` is a referenced rule (channel/HIDDEN filtered;
        // 'hello' is a string; [a-z] is a char class). `channel` is
        // also filtered as a meta-keyword.
        assert_eq!(rules[0].name, "X");
        assert_eq!(rules[0].refs, vec!["Y"]);
    }

    #[test]
    fn determinism() {
        // Running render twice on the same input yields the same bytes.
        let src = r"
grammar T;
a : b | c ;
b : 'b' ;
c : 'c' ;
";
        let r1 = parse_rules(src).unwrap();
        let r2 = parse_rules(src).unwrap();
        assert_eq!(render_json(&r1), render_json(&r2));
        assert_eq!(render_md(&r1), render_md(&r2));
    }
}
