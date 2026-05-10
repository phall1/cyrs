//! HIR property tests (spec 0001 §17.3).
//!
//! Property implemented here:
//!
//! - **P17.3.6 HIR round-trip** — `lower(parse(s)) == lower(parse(fmt(s)))`
//!   for inputs that parse cleanly.  The HIR must be invariant under
//!   formatting — i.e. HIR depends only on semantic content, not trivia
//!   (spec §17.3 P17.3.6).
//!
//! "Structural equality" is defined as equality of the *semantic signature*:
//! a span-free, HirId-free string serialisation of the clause tree and
//! expression shapes. `TextRange` and `HirId` fields are explicitly
//! excluded because formatting shifts byte offsets.
//!
//! Inputs that contain ERROR nodes after parsing are skipped via
//! `prop_assume!` — the property only applies to valid sources (per spec
//! wording "for any valid `s`").

use cypher_fmt::format;
use cypher_hir::{
    Clause, Direction, Expr, ListPredKind, MapProjectionItem, Pattern, PatternElement, PatternPart,
    Projection, RelLength, RemoveItem, SetItem, Statement, VarId, YieldItem,
    desugar::desugar_statement, lower::lower_statement,
};
use cypher_syntax::{SyntaxKind, parse};
use proptest::prelude::*;
use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Semantic signature — span-free, HirId-free serialisation
// ---------------------------------------------------------------------------
//
// The sig functions mirror the HIR tree structure but emit only the
// semantically meaningful parts (operator shapes, variable ids, literals,
// clause kinds, labels, types, directions, etc.) while omitting all
// `TextRange`, `HirId`, and `SyntaxNode` fields.

fn sig_expr(e: &Expr, out: &mut String) {
    match e {
        Expr::Null => out.push_str("Null"),
        Expr::Bool(b) => write!(out, "Bool({b})").unwrap(),
        Expr::Int(i) => write!(out, "Int({i})").unwrap(),
        Expr::Float(f) => write!(out, "Float({f})").unwrap(),
        Expr::String(s) => write!(out, "Str({s:?})").unwrap(),
        Expr::Var(VarId(v)) => write!(out, "Var({v})").unwrap(),
        Expr::Param(p) => write!(out, "Param({p})").unwrap(),
        Expr::Unresolved(n) => write!(out, "Unresolved({n})").unwrap(),
        Expr::Prop { target, prop } => {
            out.push_str("Prop(");
            sig_expr(target, out);
            write!(out, ".{prop})").unwrap();
        }
        Expr::Index { target, index } => {
            out.push_str("Index(");
            sig_expr(target, out);
            out.push(',');
            sig_expr(index, out);
            out.push(')');
        }
        Expr::Slice { target, start, end } => {
            out.push_str("Slice(");
            sig_expr(target, out);
            out.push(',');
            if let Some(s) = start {
                sig_expr(s, out);
            } else {
                out.push('_');
            }
            out.push(',');
            if let Some(e) = end {
                sig_expr(e, out);
            } else {
                out.push('_');
            }
            out.push(')');
        }
        Expr::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_expr(item, out);
            }
            out.push(']');
        }
        Expr::Map(pairs) => {
            out.push('{');
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write!(out, "{k}:").unwrap();
                sig_expr(v, out);
            }
            out.push('}');
        }
        Expr::Call {
            name,
            args,
            distinct,
        } => {
            write!(out, "Call({name},{distinct},[").unwrap();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_expr(a, out);
            }
            out.push_str("])");
        }
        Expr::BinOp { op, lhs, rhs } => {
            write!(out, "Bin({op:?},").unwrap();
            sig_expr(lhs, out);
            out.push(',');
            sig_expr(rhs, out);
            out.push(')');
        }
        Expr::UnaryOp { op, operand } => {
            write!(out, "Un({op:?},").unwrap();
            sig_expr(operand, out);
            out.push(')');
        }
        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            out.push_str("Case(");
            if let Some(s) = scrutinee {
                sig_expr(s, out);
            } else {
                out.push('_');
            }
            for (cond, then) in arms {
                out.push_str(",arm(");
                sig_expr(cond, out);
                out.push(',');
                sig_expr(then, out);
                out.push(')');
            }
            if let Some(o) = otherwise {
                out.push_str(",else(");
                sig_expr(o, out);
                out.push(')');
            }
            out.push(')');
        }
        Expr::IsNull { operand, negated } => {
            write!(out, "IsNull({negated},").unwrap();
            sig_expr(operand, out);
            out.push(')');
        }
        Expr::InList { operand, list } => {
            out.push_str("In(");
            sig_expr(operand, out);
            out.push(',');
            sig_expr(list, out);
            out.push(')');
        }
        Expr::PatternPredicate(pat) => {
            out.push_str("PatPred(");
            sig_pattern(pat, out);
            out.push(')');
        }
        Expr::ListComprehension {
            filter_var: VarId(fv),
            iterable,
            filter,
            map_expr,
        } => {
            write!(out, "ListComp({fv},").unwrap();
            sig_expr(iterable, out);
            out.push(',');
            if let Some(f) = filter {
                sig_expr(f, out);
            } else {
                out.push('_');
            }
            out.push(',');
            sig_expr(map_expr, out);
            out.push(')');
        }
        Expr::ListPredicate {
            kind,
            var: VarId(v),
            iterable,
            predicate,
        } => {
            let tag = match kind {
                ListPredKind::Any => "Any",
                ListPredKind::All => "All",
                ListPredKind::None => "None",
                ListPredKind::Single => "Single",
                // `#[non_exhaustive]`: future kinds fall back to a
                // deterministic placeholder so the signature stays
                // stable for round-trip.
                _ => "?",
            };
            write!(out, "ListPred({tag},{v},").unwrap();
            sig_expr(iterable, out);
            out.push(',');
            if let Some(p) = predicate {
                sig_expr(p, out);
            } else {
                out.push('_');
            }
            out.push(')');
        }
        Expr::MapProjection { base, items } => {
            out.push_str("MapProj(");
            sig_expr(base, out);
            for item in items {
                out.push(',');
                match item {
                    MapProjectionItem::PropCopy { prop } => {
                        write!(out, "CopyProp({prop})").unwrap();
                    }
                    MapProjectionItem::Computed { key, value } => {
                        write!(out, "Computed({key},").unwrap();
                        sig_expr(value, out);
                        out.push(')');
                    }
                    MapProjectionItem::VarShorthand {
                        var: VarId(v),
                        name,
                    } => {
                        write!(out, "VarShort({v},{name})").unwrap();
                    }
                    MapProjectionItem::Aliased { key, value } => {
                        write!(out, "Aliased({key},").unwrap();
                        sig_expr(value, out);
                        out.push(')');
                    }
                }
            }
            out.push(')');
        }
    }
}

fn sig_pattern_element(el: &PatternElement, out: &mut String) {
    match el {
        PatternElement::Node {
            bind,
            labels,
            props,
            ..
        } => {
            out.push_str("Node(");
            if let Some(VarId(v)) = bind {
                write!(out, "{v}").unwrap();
            } else {
                out.push('_');
            }
            for l in labels {
                write!(out, ":{l}").unwrap();
            }
            if let Some(p) = props {
                out.push('{');
                sig_expr(p, out);
                out.push('}');
            }
            out.push(')');
        }
        PatternElement::Rel {
            bind,
            types,
            direction,
            length,
            props,
            ..
        } => {
            out.push_str("Rel(");
            if let Some(VarId(v)) = bind {
                write!(out, "{v}").unwrap();
            } else {
                out.push('_');
            }
            for t in types {
                write!(out, ":{t}").unwrap();
            }
            let dir = match direction {
                Direction::Outgoing => "->",
                Direction::Incoming => "<-",
                Direction::Undirected => "--",
                // `Direction` is `#[non_exhaustive]` (cy-2i9.1).
                _ => "?",
            };
            out.push_str(dir);
            // `RelLength` is `#[non_exhaustive]` (cy-2i9.1).
            #[allow(clippy::match_same_arms)]
            match length {
                RelLength::Single => {}
                RelLength::Variable { min, max } => write!(out, "*{min:?}..{max:?}").unwrap(),
                _ => {}
            }
            if let Some(p) = props {
                out.push('{');
                sig_expr(p, out);
                out.push('}');
            }
            out.push(')');
        }
    }
}

fn sig_pattern_part(part: &PatternPart, out: &mut String) {
    if let Some(VarId(v)) = part.named_as {
        write!(out, "Path({v})=").unwrap();
    }
    for el in &part.elements {
        sig_pattern_element(el, out);
    }
}

fn sig_pattern(pat: &Pattern, out: &mut String) {
    out.push_str("Pat(");
    for (i, part) in pat.parts.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        sig_pattern_part(part, out);
    }
    out.push(')');
}

fn sig_projection(proj: &Projection, out: &mut String) {
    out.push_str("Proj(");
    sig_expr(&proj.expr, out);
    if let Some(alias) = &proj.alias {
        write!(out, " AS {alias}").unwrap();
    }
    out.push(')');
}

fn sig_set_item(item: &SetItem, out: &mut String) {
    match item {
        SetItem::Property {
            target,
            prop,
            value,
        } => {
            out.push_str("SetProp(");
            sig_expr(target, out);
            write!(out, ".{prop}=").unwrap();
            sig_expr(value, out);
            out.push(')');
        }
        SetItem::Labels {
            target: VarId(v),
            labels,
        } => {
            write!(out, "SetLabels({v}:{labels:?})").unwrap();
        }
        SetItem::AssignMap {
            target: VarId(v),
            map,
            replace,
        } => {
            write!(out, "SetMap({v},{replace},").unwrap();
            sig_expr(map, out);
            out.push(')');
        }
    }
}

fn sig_remove_item(item: &RemoveItem, out: &mut String) {
    match item {
        RemoveItem::Property { target, prop } => {
            out.push_str("RemProp(");
            sig_expr(target, out);
            write!(out, ".{prop})").unwrap();
        }
        RemoveItem::Labels {
            target: VarId(v),
            labels,
        } => {
            write!(out, "RemLabels({v}:{labels:?})").unwrap();
        }
    }
}

fn sig_yield_item(item: &YieldItem, out: &mut String) {
    write!(out, "Yield({}", item.name).unwrap();
    if let Some(a) = &item.alias {
        write!(out, " AS {a}").unwrap();
    }
    out.push(')');
}

fn sig_clause(clause: &Clause, out: &mut String) {
    match clause {
        Clause::Match {
            optional, pattern, ..
        } => {
            write!(out, "Match(opt={optional},").unwrap();
            sig_pattern(pattern, out);
            out.push(')');
        }
        Clause::Where { predicate, .. } => {
            out.push_str("Where(");
            sig_expr(predicate, out);
            out.push(')');
        }
        Clause::With {
            projections,
            filter,
            ..
        } => {
            out.push_str("With([");
            for (i, p) in projections.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_projection(p, out);
            }
            out.push_str("],");
            if let Some(f) = filter {
                sig_expr(f, out);
            } else {
                out.push('_');
            }
            out.push(')');
        }
        Clause::Return {
            projections,
            distinct,
            ..
        } => {
            write!(out, "Return(distinct={distinct},[").unwrap();
            for (i, p) in projections.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_projection(p, out);
            }
            out.push_str("])");
        }
        Clause::Unwind {
            list,
            bind: VarId(v),
            ..
        } => {
            write!(out, "Unwind(var={v},").unwrap();
            sig_expr(list, out);
            out.push(')');
        }
        Clause::Create { pattern, .. } => {
            out.push_str("Create(");
            sig_pattern(pattern, out);
            out.push(')');
        }
        Clause::Merge {
            pattern,
            on_create,
            on_match,
            ..
        } => {
            out.push_str("Merge(");
            sig_pattern(pattern, out);
            out.push_str(",on_create=[");
            for (i, s) in on_create.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_set_item(s, out);
            }
            out.push_str("],on_match=[");
            for (i, s) in on_match.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_set_item(s, out);
            }
            out.push_str("])");
        }
        Clause::Set { items, .. } => {
            out.push_str("Set([");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_set_item(item, out);
            }
            out.push_str("])");
        }
        Clause::Remove { items, .. } => {
            out.push_str("Remove([");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_remove_item(item, out);
            }
            out.push_str("])");
        }
        Clause::Delete {
            targets, detach, ..
        } => {
            write!(out, "Delete(detach={detach},[").unwrap();
            for (i, t) in targets.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_expr(t, out);
            }
            out.push_str("])");
        }
        Clause::Call {
            procedure,
            args,
            yields,
            ..
        } => {
            write!(out, "Call({procedure},[").unwrap();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_expr(a, out);
            }
            out.push_str("],[");
            for (i, y) in yields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                sig_yield_item(y, out);
            }
            out.push_str("])");
        }
    }
}

/// Compute the semantic signature of a [`Statement`]: span-free, HirId-free.
fn semantic_sig(stmt: &Statement) -> String {
    let mut out = String::new();
    for (i, clause) in stmt.clauses.iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        sig_clause(clause, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` when the parse tree for `src` contains no ERROR nodes
/// and no recorded [`cypher_syntax::SyntaxError`]s.
fn has_no_errors(src: &str) -> bool {
    let p = parse(src);
    if !p.errors().is_empty() {
        return false;
    }
    let root = p.syntax();
    !root.preorder_with_tokens().any(|ev| match ev {
        rowan::WalkEvent::Enter(rowan::NodeOrToken::Node(n)) => n.kind() == SyntaxKind::ERROR,
        rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(t)) => t.kind() == SyntaxKind::ERROR,
        _ => false,
    })
}

/// Lower and desugar a source string into an HIR Statement.
fn lower(src: &str) -> Statement {
    let stmt = lower_statement(src);
    desugar_statement(stmt)
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn cypher_valid() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("MATCH (n) RETURN n".to_string()),
        Just("MATCH (n:Person) RETURN n".to_string()),
        Just("MATCH (n)-[r]->(m) RETURN n, r, m".to_string()),
        Just("MATCH (n)-[r:KNOWS]->(m) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.age > 21 RETURN n".to_string()),
        Just("MATCH (n) RETURN n.name, n.age".to_string()),
        Just("MATCH (n) RETURN DISTINCT n".to_string()),
        Just("MATCH (n) RETURN n ORDER BY n.name ASC".to_string()),
        Just("MATCH (n) RETURN n SKIP 10 LIMIT 5".to_string()),
        Just("MATCH (n) WITH n RETURN n".to_string()),
        Just("MATCH (n) WITH n WHERE n.age > 21 RETURN n".to_string()),
        Just("MATCH (n) WITH n, n.age AS a WHERE a > 21 RETURN n".to_string()),
        Just("MATCH (n) WITH n WHERE n.age > 21 ORDER BY n.name LIMIT 10 RETURN n".to_string()),
        Just("MATCH (n) RETURN n;\nMATCH (m) RETURN m".to_string()),
        Just("MATCH (n {name: 'Alice'}) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.a > 1 AND n.b < 2 RETURN n".to_string()),
        Just("MATCH (n) WHERE n.a > 1 OR n.b < 2 RETURN n".to_string()),
        Just("// comment\nMATCH (n) RETURN n".to_string()),
        Just("/* block */\nMATCH (n) RETURN n".to_string()),
        Just("MATCH (n) /* inline */ RETURN n".to_string()),
        // cy-7s6.1 — list indexing + slicing shape matrix.
        Just("UNWIND [1, 2, 3] AS xs RETURN xs[0]".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN xs[-1]".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN xs[0..3]".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN xs[..3]".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN xs[0..]".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN xs[0..3][0]".to_string()),
        // cy-8x5 — list predicates: ANY / ALL / NONE / SINGLE.
        Just("UNWIND [1, 2, 3] AS xs RETURN ANY(x IN xs WHERE x > 0)".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN ALL(x IN xs WHERE x > 0)".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN NONE(x IN xs WHERE x > 0)".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN SINGLE(x IN xs WHERE x = 1)".to_string()),
        Just("UNWIND [1, 2, 3] AS xs RETURN ALL(x IN xs)".to_string()),
        // cy-5gh — list comprehension shape matrix (spec §19).
        Just("RETURN [x IN [1, 2, 3]]".to_string()),
        Just("RETURN [x IN [1, 2, 3] WHERE x > 1]".to_string()),
        Just("RETURN [x IN [1, 2, 3] | x + 1]".to_string()),
        Just("RETURN [x IN [1, 2, 3] WHERE x > 1 | x + 1]".to_string()),
    ]
}

// ---------------------------------------------------------------------------
// P17.3.6 — HIR round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        max_global_rejects: 4096,
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// `lower(parse(s))` and `lower(parse(fmt(s)))` have the same semantic
    /// signature for all valid inputs (spec P17.3.6).
    #[test]
    fn hir_round_trip_valid_cypher(s in cypher_valid()) {
        prop_assume!(has_no_errors(&s));
        let formatted = format(&s);
        prop_assume!(has_no_errors(&formatted));

        let sig_original = semantic_sig(&lower(&s));
        let sig_formatted = semantic_sig(&lower(&formatted));

        prop_assert_eq!(
            &sig_original,
            &sig_formatted,
            "HIR round-trip failed:\n  original: {:?}\n  formatted: {:?}",
            s,
            formatted,
        );
    }
}

// ---------------------------------------------------------------------------
// Unit regression guards
// ---------------------------------------------------------------------------

/// Simplest round-trip: a basic MATCH RETURN.
#[test]
fn regression_basic_match_return() {
    let s = "MATCH (n) RETURN n";
    let fmt_s = format(s);
    assert_eq!(
        semantic_sig(&lower(s)),
        semantic_sig(&lower(&fmt_s)),
        "HIR round-trip failed for basic MATCH RETURN"
    );
}

/// Round-trip with a WHERE clause.
#[test]
fn regression_where_clause() {
    let s = "MATCH (n) WHERE n.age > 21 RETURN n";
    let fmt_s = format(s);
    assert_eq!(
        semantic_sig(&lower(s)),
        semantic_sig(&lower(&fmt_s)),
        "HIR round-trip failed for WHERE clause"
    );
}

/// cy-v31: WHERE after WITH must populate `Clause::With.filter`.
/// The parser nests `WHERE_CLAUSE` inside `RETURN_BODY`; lowering must
/// look through that wrapper, not only at direct children of
/// `WITH_CLAUSE`.
#[test]
fn regression_with_where_filter_populated() {
    let stmt = lower(
        "MATCH (a) UNWIND a.aliases AS alias \
                      WITH a, alias WHERE alias CONTAINS 'Fancy' \
                      RETURN DISTINCT a.canonical_name",
    );
    let with_filter = stmt.clauses.iter().find_map(|c| match c {
        Clause::With { filter, .. } => Some(filter.as_ref()),
        _ => None,
    });
    assert!(
        matches!(with_filter, Some(Some(_))),
        "WITH ... WHERE ... must lower to Clause::With {{ filter: Some(_) }}; got {with_filter:?}"
    );
}

/// Round-trip with a comment (trivia that formatting may reformat).
#[test]
fn regression_with_comment() {
    let s = "// comment\nMATCH (n) RETURN n";
    let fmt_s = format(s);
    assert_eq!(
        semantic_sig(&lower(s)),
        semantic_sig(&lower(&fmt_s)),
        "HIR round-trip failed for source with comment"
    );
}

/// cy-b5b: `MATCH p = shortestPath((a)-[:KNOWS*]->(b))` must lower to
/// the same HIR Pattern as a plain path with a path binder. The
/// shortest-path discriminant is a CST-level annotation; HIR sees a
/// single PatternPart with `named_as = Some(p)` and the inner
/// node-rel-node element list. This guards the `lower_pattern_wrapper`
/// branch that descends through SHORTEST_PATH_PATTERN.
#[test]
fn regression_shortest_path_lowers_to_named_pattern_part() {
    let stmt = lower(
        "MATCH p = shortestPath((a)-[:KNOWS*]->(b)) RETURN p",
    );
    let parts = stmt.clauses.iter().find_map(|c| match c {
        Clause::Match { pattern, .. } => Some(&pattern.parts),
        _ => None,
    });
    let parts = parts.expect("MATCH clause present");
    assert_eq!(parts.len(), 1, "shortestPath wraps a single path part");
    let part = &parts[0];
    assert!(part.named_as.is_some(), "path binder `p` must produce named_as");
    // Inner shape: NODE, REL, NODE.
    assert_eq!(part.elements.len(), 3, "(a)-[*]->(b) is three elements");
}

/// cy-b5b: `allShortestPaths(...)` lowers identically to `shortestPath(...)`
/// at the HIR layer (the discriminant is preserved in the CST and
/// surfaced by the plan / pretty layers).
#[test]
fn regression_all_shortest_paths_lowers_to_pattern_part() {
    let stmt = lower(
        "MATCH p = allShortestPaths((a)-[:KNOWS*1..3]->(b)) RETURN p",
    );
    let part_count = stmt
        .clauses
        .iter()
        .filter_map(|c| match c {
            Clause::Match { pattern, .. } => Some(pattern.parts.len()),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(part_count, 1);
}
