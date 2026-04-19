#![allow(clippy::doc_markdown)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::manual_assert)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::format_push_string)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::collapsible_if)]

//! Codegen: turn `crates/cypher-ast/cypher.ungrammar` into
//! `crates/cypher-ast/src/generated.rs` (spec 0001 §5.1, §5.2).
//!
//! The generator emits zero-cost wrapper structs around
//! [`cypher_syntax::SyntaxNode`] — the rust-analyzer pattern. One wrapper
//! per named production whose `SyntaxKind` variant exists in
//! `cypher-syntax::kind`. Productions whose variant is missing get listed
//! in a `SKIPPED` comment block at the top of the generated file (and on
//! stderr during codegen) — they become live as more of `kind.rs` and the
//! parser fill in.
//!
//! Scope notes:
//! - cy-dpq emitted plain wrapper structs for productions whose rule is a
//!   sequence / repetition / single-node shape.
//! - cy-pbx extends the generator to emit sum-type enum wrappers for
//!   alternation-only productions whose arms are all named nodes, e.g.
//!   `Expr = Literal | BinaryExpr | …`. Alternations containing tokens,
//!   sequences, or repetitions are left skipped — those are rare and the
//!   parser flattens them anyway.
//! - Token accessors are emitted for meaningfully-typed tokens (keywords,
//!   literals, `IDENT`/`QUOTED_IDENT`). Generic punctuation like `(`, `,`
//!   is ignored — clients never ask for "the comma child".

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use ungrammar::{Grammar, Rule};

/// Entry point called from `xtask::main`. Regenerates the checked-in
/// `crates/cypher-ast/src/generated.rs`.
pub fn run() -> Result<()> {
    let out_path = ast_generated_path()?;
    let ungrammar_path = ast_ungrammar_path()?;

    let source = std::fs::read_to_string(&ungrammar_path)
        .with_context(|| format!("reading {}", ungrammar_path.display()))?;

    let report = build_generated_rs_from(&source)?;

    if !report.skipped.is_empty() {
        eprintln!(
            "[xtask codegen] skipped {} production(s):",
            report.skipped.len()
        );
        for entry in &report.skipped {
            eprintln!("  - {} : {}", entry.name, entry.reason);
        }
    }

    eprintln!(
        "[xtask codegen] parsed {} productions; emitted {} struct wrappers, \
         {} enum wrappers; skipped {}.",
        report.parsed,
        report.emitted,
        report.emitted_enums,
        report.skipped.len()
    );

    std::fs::write(&out_path, &report.source)
        .with_context(|| format!("writing {}", out_path.display()))?;

    eprintln!("[xtask codegen] wrote {}", out_path.display());
    Ok(())
}

/// Build the contents of `generated.rs` from the canonical ungrammar file
/// on disk. Pure wrapper around [`build_generated_rs_from`] that the drift
/// test and `run` both call.
pub fn build_generated_rs() -> Result<String> {
    let source = std::fs::read_to_string(ast_ungrammar_path()?)?;
    Ok(build_generated_rs_from(&source)?.source)
}

/// The output of a codegen pass.
#[derive(Debug)]
pub struct CodegenReport {
    /// Final `generated.rs` contents, already run through `rustfmt`.
    pub source: String,
    /// Number of productions parsed from the ungrammar.
    pub parsed: usize,
    /// Number of wrapper structs emitted.
    pub emitted: usize,
    /// Number of sum-type enum wrappers emitted.
    pub emitted_enums: usize,
    /// Productions skipped, with a one-line reason each.
    pub skipped: Vec<SkippedProduction>,
}

#[derive(Debug, Clone)]
pub struct SkippedProduction {
    pub name: String,
    pub reason: String,
}

/// Build generated.rs contents from an in-memory ungrammar string. Split
/// out from [`run`] so unit tests can exercise the generator without
/// touching the filesystem.
pub fn build_generated_rs_from(ungrammar_src: &str) -> Result<CodegenReport> {
    // The `ungrammar` crate only recognises `?` and `*` suffixes. Cypher's
    // ungrammar uses `+` (one-or-more) in several places. For AST codegen
    // the only thing that matters is the child type and whether it can
    // occur multiply — so rewrite `X+` into `X X*`, preserving whitespace.
    // The parser enforces the "one-or-more" semantics independently.
    //
    // ungrammar also treats every identifier as a Node reference — tokens
    // are only quoted strings. Our grammar uses bare SCREAMING_SNAKE names
    // (`INT_NUMBER`, `IDENT`, …) as token references. Quote them so the
    // crate treats them as `Rule::Token` with that name.
    let normalised = quote_screaming_snake_idents(&desugar_one_or_more(ungrammar_src));

    let grammar: Grammar = normalised
        .parse()
        .map_err(|e| anyhow!("parsing ungrammar: {e}"))?;

    let kind_variants = known_syntax_node_kinds();

    // Pass 1: classify every production as struct-shaped, enum-shaped
    // (alternation over named nodes), or skipped. Struct-shaped
    // productions still need a live `SyntaxKind` variant in
    // `cypher-syntax::kind` to be emittable; enum-shaped productions have
    // no `SyntaxKind` of their own and cast by cascading into their arm
    // wrappers, so they don't gate on `kind.rs`.
    let mut skipped: Vec<SkippedProduction> = Vec::new();
    let mut struct_names: BTreeSet<String> = BTreeSet::new();
    let mut enum_candidates: Vec<EnumCandidate> = Vec::new();
    let mut parsed_count = 0usize;

    for node_ref in grammar.iter() {
        parsed_count += 1;
        let node = &grammar[node_ref];
        let name = node.name.clone();
        let kind = pascal_to_screaming_snake(&name);

        if is_pure_alternation(&node.rule) {
            match classify_alternation(&grammar, &node.rule) {
                AlternationShape::NodesOnly(arm_names) => {
                    enum_candidates.push(EnumCandidate {
                        name: name.clone(),
                        arm_names,
                    });
                }
                AlternationShape::Mixed(reason) => {
                    skipped.push(SkippedProduction {
                        name: name.clone(),
                        reason,
                    });
                }
            }
            continue;
        }

        if !kind_variants.contains(kind.as_str()) {
            skipped.push(SkippedProduction {
                name: name.clone(),
                reason: format!(
                    "no `SyntaxKind::{kind}` variant in cypher-syntax::kind (see cy-nom follow-ups)"
                ),
            });
            continue;
        }

        struct_names.insert(name);
    }

    // Iterate to fixed point: an enum survives iff it has at least one
    // arm whose target type is a struct wrapper we're emitting, or
    // another enum that itself survived. Enums that lose all their arms
    // get demoted to skipped entries below (they'll light up as more
    // `SyntaxKind` variants land in `kind.rs`).
    let mut live_enum_names: BTreeSet<String> =
        enum_candidates.iter().map(|c| c.name.clone()).collect();
    loop {
        let before = live_enum_names.len();
        live_enum_names = enum_candidates
            .iter()
            .filter(|cand| live_enum_names.contains(&cand.name))
            .filter(|cand| {
                cand.arm_names
                    .iter()
                    .any(|arm| struct_names.contains(arm) || live_enum_names.contains(arm))
            })
            .map(|cand| cand.name.clone())
            .collect();
        if live_enum_names.len() == before {
            break;
        }
    }

    // The set of type names that will appear in the generated module.
    // Child accessors / enum arms referring to anything else are dropped
    // so the file compiles.
    let mut emittable_type_names: BTreeSet<String> = struct_names.clone();
    emittable_type_names.extend(live_enum_names.iter().cloned());

    // Pass 2: for every emitted struct production, collect child accessors
    // whose wrapper target is also emitted. Accessors pointing at skipped
    // types are omitted — the generated file must compile.
    let mut emitted_wrappers: Vec<EmittedWrapper> = Vec::new();
    for node_ref in grammar.iter() {
        let node = &grammar[node_ref];
        if !struct_names.contains(node.name.as_str()) {
            continue;
        }
        let kind = pascal_to_screaming_snake(&node.name);
        let mut children = collect_children(&grammar, &node.rule);
        children
            .nodes
            .retain(|c| emittable_type_names.contains(c.ty.as_str()));
        emitted_wrappers.push(EmittedWrapper {
            name: node.name.clone(),
            kind,
            children,
        });
    }

    // Pass 3: materialise sum-type enums. Candidates that lost all their
    // arms during the fixed-point loop above get moved to `skipped`;
    // survivors keep only the arms whose target type is emittable.
    let mut emitted_enums: Vec<EmittedEnum> = Vec::new();
    for cand in enum_candidates {
        if !live_enum_names.contains(&cand.name) {
            skipped.push(SkippedProduction {
                name: cand.name,
                reason: "alternation arms all reference types that aren't emitted yet \
                         (will light up as `SyntaxKind` variants land)"
                    .to_string(),
            });
            continue;
        }
        let variants: Vec<String> = cand
            .arm_names
            .iter()
            .filter(|arm| emittable_type_names.contains(arm.as_str()))
            .cloned()
            .collect();
        emitted_enums.push(EmittedEnum {
            name: cand.name,
            variants,
        });
    }

    let source = render_file(&emitted_wrappers, &emitted_enums, &skipped);
    let formatted = rustfmt(&source).unwrap_or_else(|err| {
        eprintln!("[xtask codegen] rustfmt unavailable ({err}); writing unformatted output");
        source
    });

    Ok(CodegenReport {
        source: formatted,
        parsed: parsed_count,
        emitted: emitted_wrappers.len(),
        emitted_enums: emitted_enums.len(),
        skipped,
    })
}

/// Convenience alias used by the unit tests in this module: identical to
/// [`build_generated_rs_from`] but returns just the rendered source string.
#[cfg(test)]
pub(crate) fn build_generated_from_str(src: &str) -> Result<String> {
    Ok(build_generated_rs_from(src)?.source)
}

/// A production we plan to emit as a sum-type enum: all alternation arms
/// resolved to `Rule::Node` references, yielding the given named arms.
#[derive(Debug)]
struct EnumCandidate {
    /// PascalCase production name.
    name: String,
    /// PascalCase type name of each alternation arm, in source order.
    arm_names: Vec<String>,
}

/// What an alternation's arms look like once we inspect them.
#[derive(Debug)]
enum AlternationShape {
    /// Every arm is a bare named node reference. `Vec` is in source order.
    NodesOnly(Vec<String>),
    /// Something else — a token, a sequence, a repetition, a labelled rule,
    /// or an option. We skip these and leave the reason for the report.
    Mixed(String),
}

// =====================================================================
// Rendering
// =====================================================================

#[derive(Debug)]
struct EmittedWrapper {
    /// PascalCase production name, used as the Rust type name.
    name: String,
    /// SCREAMING_SNAKE_CASE `SyntaxKind` variant.
    kind: String,
    /// Child accessors (nodes + tokens).
    children: Children,
}

#[derive(Debug)]
struct EmittedEnum {
    /// PascalCase production name, used as the Rust enum name.
    name: String,
    /// PascalCase arm type names, in source order. Each becomes a
    /// tuple-variant wrapping the arm's typed wrapper.
    variants: Vec<String>,
}

#[derive(Debug, Default)]
struct Children {
    /// Each entry: (accessor_fn_name, wrapper_type, cardinality).
    nodes: Vec<NodeChild>,
    /// Each entry: (accessor_fn_name, SyntaxKind variant).
    tokens: Vec<TokenChild>,
}

#[derive(Debug)]
struct NodeChild {
    fn_name: String,
    ty: String,
    many: bool,
}

#[derive(Debug)]
struct TokenChild {
    fn_name: String,
    kind: String,
}

fn render_file(
    wrappers: &[EmittedWrapper],
    enums: &[EmittedEnum],
    skipped: &[SkippedProduction],
) -> String {
    let mut out = String::new();
    out.push_str("// @generated by `cargo xtask codegen`. DO NOT EDIT.\n");
    out.push_str("// Source: crates/cypher-ast/cypher.ungrammar (spec 0001 §5.2).\n");
    out.push_str("//\n");
    out.push_str("// Typed AST wrappers are zero-cost views over `cypher_syntax::SyntaxNode`\n");
    out.push_str("// (spec §5.1). Each struct wrapper has a `cast` that succeeds iff the\n");
    out.push_str("// underlying node's `SyntaxKind` matches, plus `Option`-returning\n");
    out.push_str("// accessors per child (spec §5.3). Sum-type enum wrappers cover\n");
    out.push_str("// alternation-only productions (spec §5.4) and cast by cascading into\n");
    out.push_str("// their variants — first successful arm cast wins.\n");
    out.push_str("//\n");
    if skipped.is_empty() {
        out.push_str("// All productions in the ungrammar were emitted.\n");
    } else {
        out.push_str("// SKIPPED (no wrapper emitted — productions listed below):\n");
        for entry in skipped {
            out.push_str(&format!("//   - {}: {}\n", entry.name, entry.reason));
        }
    }
    out.push('\n');

    out.push_str("#![forbid(unsafe_code)]\n");
    out.push_str("#![allow(clippy::wildcard_imports)]\n");
    out.push_str("#![allow(clippy::return_self_not_must_use)]\n");
    out.push_str("#![allow(dead_code)]\n");
    out.push('\n');
    out.push_str("use cypher_syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};\n");
    out.push('\n');

    for w in wrappers {
        render_wrapper(&mut out, w);
        out.push('\n');
    }

    for e in enums {
        render_enum(&mut out, e);
        out.push('\n');
    }

    out
}

fn render_wrapper(out: &mut String, w: &EmittedWrapper) {
    out.push_str("#[derive(Debug, Clone, PartialEq, Eq, Hash)]\n");
    out.push_str(&format!("pub struct {} {{\n", w.name));
    out.push_str("    syntax: SyntaxNode,\n");
    out.push_str("}\n");
    out.push('\n');
    out.push_str(&format!("impl {} {{\n", w.name));
    out.push_str("    pub fn cast(syntax: SyntaxNode) -> Option<Self> {\n");
    out.push_str(&format!(
        "        (syntax.kind() == SyntaxKind::{}).then_some(Self {{ syntax }})\n",
        w.kind
    ));
    out.push_str("    }\n");
    out.push('\n');
    out.push_str("    pub fn syntax(&self) -> &SyntaxNode {\n");
    out.push_str("        &self.syntax\n");
    out.push_str("    }\n");

    for child in &w.children.nodes {
        out.push('\n');
        if child.many {
            out.push_str(&format!(
                "    pub fn {}(&self) -> impl Iterator<Item = {}> + '_ {{\n",
                child.fn_name, child.ty
            ));
            out.push_str(&format!(
                "        self.syntax.children().filter_map({}::cast)\n",
                child.ty
            ));
            out.push_str("    }\n");
        } else {
            out.push_str(&format!(
                "    pub fn {}(&self) -> Option<{}> {{\n",
                child.fn_name, child.ty
            ));
            out.push_str(&format!(
                "        self.syntax.children().find_map({}::cast)\n",
                child.ty
            ));
            out.push_str("    }\n");
        }
    }

    for tok in &w.children.tokens {
        out.push('\n');
        out.push_str(&format!(
            "    pub fn {}(&self) -> Option<SyntaxToken> {{\n",
            tok.fn_name
        ));
        out.push_str("        self.syntax\n");
        out.push_str("            .children_with_tokens()\n");
        out.push_str("            .filter_map(SyntaxElement::into_token)\n");
        out.push_str(&format!(
            "            .find(|t| t.kind() == SyntaxKind::{})\n",
            tok.kind
        ));
        out.push_str("    }\n");
    }

    out.push_str("}\n");
}

fn render_enum(out: &mut String, e: &EmittedEnum) {
    out.push_str("#[derive(Debug, Clone, PartialEq, Eq, Hash)]\n");
    out.push_str(&format!("pub enum {} {{\n", e.name));
    for v in &e.variants {
        out.push_str(&format!("    {v}({v}),\n"));
    }
    out.push_str("}\n");
    out.push('\n');

    out.push_str(&format!("impl {} {{\n", e.name));
    out.push_str("    pub fn cast(syntax: SyntaxNode) -> Option<Self> {\n");
    // Try each variant's cast in source order; first match wins. Clone
    // the syntax handle for every try except the last — cheap, it's an
    // Rc — so each `cast` gets a fresh owned value.
    for (i, v) in e.variants.iter().enumerate() {
        let arg = if i + 1 == e.variants.len() {
            "syntax".to_string()
        } else {
            "syntax.clone()".to_string()
        };
        out.push_str(&format!(
            "        if let Some(inner) = {v}::cast({arg}) {{\n"
        ));
        out.push_str(&format!("            return Some(Self::{v}(inner));\n"));
        out.push_str("        }\n");
    }
    out.push_str("        None\n");
    out.push_str("    }\n");
    out.push('\n');
    out.push_str("    pub fn syntax(&self) -> &SyntaxNode {\n");
    out.push_str("        match self {\n");
    for v in &e.variants {
        out.push_str(&format!(
            "            Self::{v}(inner) => inner.syntax(),\n"
        ));
    }
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");
}

// =====================================================================
// Ungrammar preprocessing
// =====================================================================

/// Rewrite every unquoted `X+` into `X X*` so the `ungrammar` crate (which
/// only supports `?` and `*`) can parse the file. Preserves quoted strings
/// (`'+'`, `'+='`) and `//` line comments verbatim. For AST codegen the
/// only thing that matters is child type and multiplicity, so the loss of
/// the "one-or-more" lower bound is harmless here; the parser enforces it.
fn desugar_one_or_more(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 32);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;

        // Pass quoted literals through unchanged.
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

        // Pass line comments through unchanged.
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

/// Wrap bare SCREAMING_SNAKE_CASE identifiers in single quotes so
/// `ungrammar` treats them as `Rule::Token` rather than forward references
/// to undefined nodes. Preserves quoted literals and `//` comments.
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
            // Walk the full identifier.
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
            // All-uppercase (optional digits/underscores), length > 1.
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

/// Pull the trailing token (an identifier, or a balanced `(...)` group)
/// off the tail of `buf`. Returns a copy; `buf` is left untouched.
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
// Rule analysis
// =====================================================================

/// Is this rule a top-level alternation?
///
/// Top-level `Rule::Alt` nodes get classified further by
/// [`classify_alternation`] to decide whether they yield a sum-type enum
/// (every arm is a named node) or get skipped (token / sequence / rep
/// arms — rare, always land in a dedicated follow-up).
fn is_pure_alternation(rule: &Rule) -> bool {
    matches!(rule, Rule::Alt(_))
}

/// Inspect a top-level alternation and decide whether every arm is a
/// bare `Rule::Node` (in which case we can emit a clean sum-type enum)
/// or something richer. Call only on rules that already passed
/// [`is_pure_alternation`].
fn classify_alternation(grammar: &Grammar, rule: &Rule) -> AlternationShape {
    let Rule::Alt(arms) = rule else {
        return AlternationShape::Mixed(
            "classify_alternation called on non-alternation (codegen bug)".to_string(),
        );
    };

    let mut names = Vec::with_capacity(arms.len());
    for arm in arms {
        match arm {
            Rule::Node(n) => {
                names.push(grammar[*n].name.clone());
            }
            Rule::Token(_) => {
                return AlternationShape::Mixed(
                    "alternation contains a token arm \
                     (sum-type-over-tokens emitter not in cy-pbx scope)"
                        .to_string(),
                );
            }
            Rule::Seq(_) => {
                return AlternationShape::Mixed(
                    "alternation contains a sequence arm (not a single-node shape)".to_string(),
                );
            }
            Rule::Rep(_) => {
                return AlternationShape::Mixed(
                    "alternation contains a repetition arm (not a single-node shape)".to_string(),
                );
            }
            Rule::Opt(_) => {
                return AlternationShape::Mixed(
                    "alternation contains an optional arm (not a single-node shape)".to_string(),
                );
            }
            Rule::Labeled { .. } => {
                return AlternationShape::Mixed(
                    "alternation contains a labelled arm (not a bare node reference)".to_string(),
                );
            }
            Rule::Alt(_) => {
                return AlternationShape::Mixed(
                    "alternation contains a nested alternation arm (ungrammar should flatten)"
                        .to_string(),
                );
            }
        }
    }
    AlternationShape::NodesOnly(names)
}

/// Walk a rule and collect accessor specs for every reachable node / token
/// that we want to expose. Labels (`field:Child`) override the default
/// accessor name.
fn collect_children(grammar: &Grammar, rule: &Rule) -> Children {
    let mut visitor = ChildVisitor::default();
    visitor.visit(grammar, rule, false, None);
    visitor.finish()
}

#[derive(Default)]
struct ChildVisitor {
    nodes: Vec<NodeChild>,
    tokens: Vec<TokenChild>,
    /// Names already allocated; avoids duplicate accessors when the grammar
    /// mentions the same child shape twice.
    seen_node_names: BTreeSet<String>,
    seen_token_names: BTreeSet<String>,
    /// Tracks `Name` → `many` for upgrading singleton accessors to iterators
    /// when a later occurrence of the same node is under a repetition.
    node_many: BTreeMap<String, bool>,
}

impl ChildVisitor {
    fn visit(&mut self, grammar: &Grammar, rule: &Rule, many: bool, label: Option<&str>) {
        match rule {
            Rule::Labeled { label, rule } => {
                self.visit(grammar, rule, many, Some(label.as_str()));
            }
            Rule::Node(n) => {
                let node = &grammar[*n];
                let type_name = node.name.clone();
                let fn_name = label
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| pascal_to_snake(&type_name));

                // If we've seen this node but under different repetitions,
                // collapse to `many = true`.
                let prev_many = self.node_many.get(&fn_name).copied();
                let effective_many = many || prev_many.unwrap_or(false);
                self.node_many.insert(fn_name.clone(), effective_many);

                if self.seen_node_names.insert(fn_name.clone()) {
                    self.nodes.push(NodeChild {
                        fn_name,
                        ty: type_name,
                        many: effective_many,
                    });
                } else if effective_many {
                    // Upgrade cardinality of the already-recorded entry.
                    if let Some(existing) = self.nodes.iter_mut().find(|c| c.fn_name == fn_name) {
                        existing.many = true;
                    }
                }
            }
            Rule::Token(t) => {
                let tok = &grammar[*t];
                if let Some((fn_name, kind)) = token_accessor(tok.name.as_str(), label) {
                    if self.seen_token_names.insert(fn_name.clone()) {
                        self.tokens.push(TokenChild { fn_name, kind });
                    }
                }
            }
            Rule::Seq(rules) => {
                for r in rules {
                    self.visit(grammar, r, many, None);
                }
            }
            Rule::Alt(rules) => {
                for r in rules {
                    self.visit(grammar, r, many, None);
                }
            }
            Rule::Opt(inner) => {
                self.visit(grammar, inner, many, label);
            }
            Rule::Rep(inner) => {
                self.visit(grammar, inner, true, label);
            }
        }
    }

    fn finish(self) -> Children {
        Children {
            nodes: self.nodes,
            tokens: self.tokens,
        }
    }
}

/// Decide whether a token referenced in the grammar deserves an accessor,
/// and if so return `(fn_name, SyntaxKind)`. Punctuation / structural
/// tokens are skipped — they carry no useful information to callers.
fn token_accessor(tok_text: &str, label: Option<&str>) -> Option<(String, String)> {
    // Tokens after ungrammar preprocessing:
    //  - pure alphabetic names (`MATCH`, `AND`, `SHORTESTPATH`) are keywords
    //    — they appeared quoted in the source.
    //  - SCREAMING_SNAKE with an underscore or digit (`INT_NUMBER`, `QUOTED_IDENT`)
    //    name a `SyntaxKind` directly — `quote_screaming_snake_idents` wrapped
    //    them so ungrammar treats them as `Rule::Token`.
    //  - symbolic text (`->`, `!=`, `+`) is punctuation — uninteresting to
    //    AST consumers, dropped.

    if !tok_text.is_empty() && tok_text.chars().all(|c| c.is_ascii_alphabetic()) {
        let upper = tok_text.to_ascii_uppercase();
        let kind = format!("{upper}_KW");
        if !known_keyword_kinds().contains(kind.as_str()) {
            return None;
        }
        let fn_name = label
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{}_token", upper.to_ascii_lowercase()));
        return Some((fn_name, kind));
    }

    let looks_like_kind_ref = tok_text.len() > 1
        && tok_text
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        && tok_text.chars().any(|c| c == '_' || c.is_ascii_digit());
    if looks_like_kind_ref {
        let kind = match tok_text {
            "IDENT" => "IDENT",
            "QUOTED_IDENT" => "QUOTED_IDENT",
            "INT_LITERAL" | "INT_NUMBER" => "INT_LITERAL",
            "FLOAT_LITERAL" | "FLOAT_NUMBER" => "FLOAT_LITERAL",
            "STRING_LITERAL" | "STRING" => "STRING_LITERAL",
            "BOOL_LITERAL" => "BOOL_LITERAL",
            "NULL_LITERAL" => "NULL_LITERAL",
            "PARAM" | "PARAMETER" => "PARAM",
            _ => return None,
        };
        let fn_name = label
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| pascal_to_snake(&kind_to_camel(kind)));
        return Some((fn_name, kind.to_string()));
    }

    None
}

// =====================================================================
// Name utilities
// =====================================================================

fn pascal_to_screaming_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn pascal_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// SCREAMING_SNAKE → PascalCase, for coining accessor names out of
/// `SyntaxKind` variant names when no grammar label is provided.
fn kind_to_camel(kind: &str) -> String {
    let mut out = String::with_capacity(kind.len());
    let mut upper_next = true;
    for c in kind.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

// =====================================================================
// Known SyntaxKind sets
// =====================================================================

/// Every `SyntaxKind` variant in the node zone
/// (`SOURCE_FILE..ERROR` per `cypher-syntax::kind`). Kept in sync by the
/// drift test in `tests/` below: if `kind.rs` gains a new node variant
/// without being listed here, the test fires.
fn known_syntax_node_kinds() -> BTreeSet<&'static str> {
    [
        "SOURCE_FILE",
        "STATEMENT",
        "MATCH_CLAUSE",
        "OPTIONAL_MATCH_CLAUSE",
        "WHERE_CLAUSE",
        "WITH_CLAUSE",
        "RETURN_CLAUSE",
        "CREATE_CLAUSE",
        "MERGE_CLAUSE",
        "SET_CLAUSE",
        "REMOVE_CLAUSE",
        "DELETE_CLAUSE",
        "UNWIND_CLAUSE",
        "CALL_CLAUSE",
        "UNION_TAIL",
        "MERGE_ACTION",
        "RETURN_BODY",
        "RETURN_ITEMS",
        "RETURN_ITEM",
        "ORDER_BY",
        "ORDER_ITEM",
        "SKIP_SUBCLAUSE",
        "LIMIT_SUBCLAUSE",
        "PATTERN",
        "PATTERN_PART",
        "NAMED_PATTERN_PART",
        "NODE_PATTERN",
        "REL_PATTERN",
        "REL_DETAIL",
        "REL_LENGTH",
        "LABEL_EXPR",
        "REL_TYPE_EXPR",
        "PROPERTY_MAP",
        "SET_ITEM",
        "REMOVE_ITEM",
        "YIELD_SUBCLAUSE",
        "YIELD_ITEM",
        "PROCEDURE_NAME",
        "BINARY_EXPR",
        "UNARY_EXPR",
        "POSTFIX_EXPR",
        "LITERAL_EXPR",
        "VAR_EXPR",
        "PROP_ACCESS_EXPR",
        "SUBSCRIPT_EXPR",
        "LIST_LITERAL",
        "MAP_LITERAL",
        "MAP_PROJECTION",
        "MAP_PROJECTION_ITEM",
        "CASE_EXPR",
        "CASE_WHEN_ARM",
        "CASE_ELSE_ARM",
        "FUNCTION_CALL",
        "CALL_ARGS",
        "PAREN_EXPR",
        "LIST_COMPREHENSION",
        "PATTERN_COMPREHENSION",
        "PATTERN_PREDICATE",
        "PARAM_EXPR",
        "IS_NULL_EXPR",
        "IN_EXPR",
        "REGEX_MATCH_EXPR",
        "STRING_OP_EXPR",
        "NAME",
        "ARG_LIST",
    ]
    .into_iter()
    .collect()
}

/// Keyword zone of `cypher-syntax::kind`. A keyword accessor is only
/// emitted if the keyword's `_KW` variant is listed here.
fn known_keyword_kinds() -> BTreeSet<&'static str> {
    [
        "MATCH_KW",
        "OPTIONAL_KW",
        "WHERE_KW",
        "WITH_KW",
        "RETURN_KW",
        "CREATE_KW",
        "MERGE_KW",
        "DELETE_KW",
        "DETACH_KW",
        "SET_KW",
        "REMOVE_KW",
        "UNWIND_KW",
        "CALL_KW",
        "YIELD_KW",
        "ON_KW",
        "AS_KW",
        "AND_KW",
        "OR_KW",
        "XOR_KW",
        "NOT_KW",
        "IN_KW",
        "IS_KW",
        "NULL_KW",
        "TRUE_KW",
        "FALSE_KW",
        "CASE_KW",
        "WHEN_KW",
        "THEN_KW",
        "ELSE_KW",
        "END_KW",
        "ORDER_KW",
        "BY_KW",
        "ASC_KW",
        "ASCENDING_KW",
        "DESC_KW",
        "DESCENDING_KW",
        "SKIP_KW",
        "LIMIT_KW",
        "DISTINCT_KW",
        "UNION_KW",
        "ALL_KW",
        "STARTS_KW",
        "ENDS_KW",
        "CONTAINS_KW",
        "DIV_KW",
        "MOD_KW",
        "COUNT_KW",
        "EXISTS_KW",
        "SHORTESTPATH_KW",
        "ALLSHORTESTPATHS_KW",
    ]
    .into_iter()
    .collect()
}

// =====================================================================
// Filesystem helpers
// =====================================================================

fn workspace_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR of xtask = <workspace>/xtask. Go up one level.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .ok_or_else(|| anyhow!("xtask has no parent directory?"))?;
    Ok(root.to_path_buf())
}

fn ast_ungrammar_path() -> Result<PathBuf> {
    Ok(workspace_root()?.join("crates/cypher-ast/cypher.ungrammar"))
}

fn ast_generated_path() -> Result<PathBuf> {
    Ok(workspace_root()?.join("crates/cypher-ast/src/generated.rs"))
}

fn rustfmt(source: &str) -> Result<String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| "spawning rustfmt")?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("rustfmt stdin unavailable"))?;
        stdin.write_all(source.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[allow(dead_code)]
fn _enforce_path_is_absolute(p: &Path) {
    assert!(p.is_absolute());
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_screaming_snake_basics() {
        assert_eq!(pascal_to_screaming_snake("NodePattern"), "NODE_PATTERN");
        assert_eq!(pascal_to_screaming_snake("Expr"), "EXPR");
        assert_eq!(pascal_to_screaming_snake("SourceFile"), "SOURCE_FILE");
    }

    #[test]
    fn pascal_snake_basics() {
        assert_eq!(pascal_to_snake("NodePattern"), "node_pattern");
        assert_eq!(pascal_to_snake("Expr"), "expr");
    }

    #[test]
    fn emits_sum_type_enum_for_alternation_of_nodes() {
        // Use arm names that already have live `SyntaxKind` variants so
        // the struct wrappers they reference actually get emitted; the
        // enum then picks them up.
        let src = "Clause = MatchClause | WithClause\n\
                   MatchClause = 'MATCH'\n\
                   WithClause = 'WITH'\n";
        let out = build_generated_from_str(src).expect("gen");
        assert!(
            out.contains("pub enum Clause"),
            "expected enum Clause in output:\n{out}"
        );
        assert!(
            out.contains("MatchClause(MatchClause)"),
            "expected MatchClause arm in output:\n{out}"
        );
        assert!(
            out.contains("WithClause(WithClause)"),
            "expected WithClause arm in output:\n{out}"
        );
        assert!(out.contains("pub fn cast"));
        assert!(out.contains("pub fn syntax"));
    }

    #[test]
    fn still_emits_struct_for_pure_sequence() {
        let src = "MatchClause = 'MATCH' WithClause\nWithClause = 'WITH'\n";
        let out = build_generated_from_str(src).expect("gen");
        assert!(
            out.contains("pub struct MatchClause"),
            "expected struct MatchClause in output:\n{out}"
        );
        assert!(
            !out.contains("pub enum MatchClause"),
            "did not expect enum MatchClause in output:\n{out}"
        );
    }

    #[test]
    fn skips_alternation_with_token_arms() {
        // `BinaryOp = '+' | '-'` — tokens in arms, not a clean node-sum.
        // The enum should NOT be emitted; the production lands in the
        // skipped list for a follow-up.
        let src = "MatchClause = 'MATCH'\nBinaryOp = '+' | '-'\n";
        let out = build_generated_from_str(src).expect("gen");
        assert!(
            !out.contains("pub enum BinaryOp"),
            "did not expect enum BinaryOp in output:\n{out}"
        );
        assert!(
            out.contains("token arm"),
            "expected a token-arm skip reason for BinaryOp:\n{out}"
        );
    }

    /// Drift gate: running the generator against the canonical ungrammar
    /// file must produce output byte-for-byte identical to the checked-in
    /// `crates/cypher-ast/src/generated.rs`.
    #[test]
    fn generated_ast_is_fresh() {
        let ungrammar = std::fs::read_to_string(ast_ungrammar_path().expect("ungrammar path"))
            .expect("read ungrammar");

        let regen = build_generated_rs_from(&ungrammar)
            .expect("codegen succeeds on a well-formed ungrammar");

        let committed = std::fs::read_to_string(ast_generated_path().expect("generated path"))
            .expect("read generated.rs");

        if regen.source != committed {
            // Keep the assertion message short; the body of the diff
            // helps a human but would drown the test log.
            panic!(
                "generated.rs is stale. Run:\n    cargo run -p xtask -- codegen\n\
                 and commit the result.\n\n\
                 parsed={}, emitted={}, skipped={}",
                regen.parsed,
                regen.emitted,
                regen.skipped.len()
            );
        }
    }
}
