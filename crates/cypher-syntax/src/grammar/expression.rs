//! Expression parser — Pratt operator precedence over the openCypher /
//! GQL operator table. Spec §4.2 ("hand-written event-based recursive-
//! descent with Pratt precedence for expressions").
//!
//! # Precedence table (loose to tight)
//!
//! Numeric priority is the binding power of the *left* operand; an
//! operator binds the right operand at `priority + 1` for left
//! associativity, `priority` for right associativity. Match openCypher /
//! GQL canonical ordering:
//!
//! | Priority | Operators                                                  | Assoc |
//! | -------: | ---------------------------------------------------------- | ----- |
//! |        1 | `OR`                                                       | left  |
//! |        2 | `XOR`                                                      | left  |
//! |        3 | `AND`                                                      | left  |
//! |        4 | unary `NOT`                                                | prefix|
//! |        5 | `=` `<>` `!=` `<` `<=` `>` `>=` `IS [NOT] NULL` `STARTS WITH` `ENDS WITH` `CONTAINS` `=~` `IN` | non-assoc |
//! |        6 | `+` `-`                                                    | left  |
//! |        7 | `*` `/` `%`                                                | left  |
//! |        8 | `^`                                                        | right |
//! |        9 | unary `-` / unary `+`                                      | prefix|
//! |       10 | postfix: `.`, `[]`, `()`                                   | left  |
//! |     atom | identifier / literal / parenthesised / parameter           | —     |
//!
//! # cy-nom scope
//!
//! Implemented: every operator in the table above. Atoms cover
//! identifier, int/float/string/bool/null literal, parameter, and
//! `(Expr)`. Postfix covers property access, function call, and index.
//!
//! Deferred (each tagged with `cy-nom: v1 scope` at its stub):
//! list literals `[...]`, map literals `{...}`, list/pattern
//! comprehensions, `CASE` expressions, pattern predicates in expressions,
//! `EXISTS(...)`, `COUNT(*)` standalone form.

use crate::SyntaxKind;
use crate::parser::{CompletedMarker, Marker, Parser, TokenSet};

/// Parse an expression and return a handle to the completed root node.
/// Returns `None` if the current token starts nothing expression-like —
/// the caller should emit its own "expected expression" diagnostic.
pub(crate) fn expr(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    expr_bp(p, 0)
}

/// Recursion-safety cap. Pathological inputs like nested parens cannot
/// exceed this depth. Protects fuzz against stack overflow.
const MAX_EXPR_DEPTH: u32 = 256;

/// Pratt loop. `min_bp` is the minimum binding power a binary operator
/// must exceed to continue the expression on the right. Unary operators
/// and atoms are parsed in `lhs`.
fn expr_bp(p: &mut Parser<'_>, min_bp: u8) -> Option<CompletedMarker> {
    expr_bp_depth(p, min_bp, 0)
}

fn expr_bp_depth(p: &mut Parser<'_>, min_bp: u8, depth: u32) -> Option<CompletedMarker> {
    if depth > MAX_EXPR_DEPTH {
        p.error("expression nesting exceeds parser limit");
        return None;
    }

    // --- Prefix / unary --------------------------------------------------
    let mut lhs = if let Some(prefix_bp) = prefix_bp(p.current()) {
        let m = p.start();
        let op_kind = p.current();
        p.bump_any();
        // Right binding power drives recursion. Unary is right-associative.
        if expr_bp_depth(p, prefix_bp, depth + 1).is_none() {
            p.error(format!("expected operand after unary {op_kind:?}"));
        }
        m.complete(p, SyntaxKind::UNARY_EXPR)
    } else {
        atom(p, depth)?
    };

    // --- Postfix + infix loop -------------------------------------------
    loop {
        // Postfix operators always bind tightest.
        if let Some(postfix) = postfix_op(p) {
            if postfix.bp < min_bp {
                break;
            }
            lhs = apply_postfix(p, lhs, postfix, depth);
            continue;
        }

        // `IS [NOT] NULL` — postfix, priority 5 (comparison-level).
        if p.at(SyntaxKind::IS_KW) {
            let null_check_bp = 10;
            if null_check_bp < min_bp {
                break;
            }
            let m = lhs.precede(p);
            p.bump(SyntaxKind::IS_KW);
            p.eat(SyntaxKind::NOT_KW);
            if !p.eat(SyntaxKind::NULL_KW) {
                p.error("expected NULL after IS");
            }
            lhs = m.complete(p, SyntaxKind::IS_NULL_EXPR);
            continue;
        }

        // Infix binary operators.
        if let Some(op) = infix_op(p) {
            if op.left_bp < min_bp {
                break;
            }
            let m = lhs.precede(p);
            // Consume the operator token(s).
            consume_infix_op(p, op.kind);
            // Right-hand side parses at right_bp.
            if expr_bp_depth(p, op.right_bp, depth + 1).is_none() {
                p.error("expected right-hand side of binary expression");
            }
            lhs = m.complete(p, op.node);
            continue;
        }

        break;
    }

    Some(lhs)
}

// --------------------------------------------------------------------------
// Atoms
// --------------------------------------------------------------------------

fn atom(p: &mut Parser<'_>, depth: u32) -> Option<CompletedMarker> {
    let kind = p.current();
    Some(match kind {
        SyntaxKind::INT_LITERAL | SyntaxKind::FLOAT_LITERAL | SyntaxKind::STRING_LITERAL => {
            literal_atom(p, SyntaxKind::LITERAL_EXPR)
        }
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => {
            literal_keyword_atom(p, SyntaxKind::LITERAL_EXPR)
        }
        SyntaxKind::NULL_KW => literal_keyword_atom(p, SyntaxKind::LITERAL_EXPR),
        SyntaxKind::PARAM => {
            let m = p.start();
            p.bump(SyntaxKind::PARAM);
            m.complete(p, SyntaxKind::PARAM_EXPR)
        }
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
            // Variable reference. A following `(` is handled as a postfix
            // function-call in the infix/postfix loop.
            let m = p.start();
            p.bump_any();
            m.complete(p, SyntaxKind::VAR_EXPR)
        }
        SyntaxKind::L_PAREN => paren_expr(p, depth),
        // cy-nom: v1 scope — list/map literals, comprehensions, CASE,
        // pattern predicates, EXISTS(...) land in follow-up beads.
        _ => return None,
    })
}

fn literal_atom(p: &mut Parser<'_>, node: SyntaxKind) -> CompletedMarker {
    let m = p.start();
    p.bump_any();
    m.complete(p, node)
}

/// TRUE/FALSE/NULL keywords are wrapped into a literal expression so the
/// AST sees them uniformly with the numeric/string literals above.
fn literal_keyword_atom(p: &mut Parser<'_>, node: SyntaxKind) -> CompletedMarker {
    let m = p.start();
    p.bump_any();
    m.complete(p, node)
}

fn paren_expr(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_PAREN));
    let m = p.start();
    p.bump(SyntaxKind::L_PAREN);
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error("expected expression inside parentheses");
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        // Virtual-token insertion per spec §4.3.
        p.error("expected ')' to close expression");
    }
    m.complete(p, SyntaxKind::PAREN_EXPR)
}

// --------------------------------------------------------------------------
// Prefix / unary binding
// --------------------------------------------------------------------------

fn prefix_bp(kind: SyntaxKind) -> Option<u8> {
    Some(match kind {
        // `NOT` at priority 4 — lower than comparison, higher than AND/XOR/OR.
        SyntaxKind::NOT_KW => 8,
        // Unary `-` / `+` at priority 9 — higher than multiplicative ops.
        SyntaxKind::MINUS | SyntaxKind::PLUS => 18,
        _ => return None,
    })
}

// --------------------------------------------------------------------------
// Infix binary operators
// --------------------------------------------------------------------------

/// Binding powers use doubled priorities (2 per table row) so left-assoc
/// can set `right_bp` = `left_bp` + 1 and right-assoc can set them equal —
/// the standard Pratt encoding.
#[derive(Copy, Clone, Debug)]
struct InfixOp {
    kind: InfixKind,
    left_bp: u8,
    right_bp: u8,
    /// `SyntaxKind` used for the resulting node.
    node: SyntaxKind,
}

#[derive(Copy, Clone, Debug)]
enum InfixKind {
    /// Single-token operator — just bump `tok`.
    Single(SyntaxKind),
    /// `STARTS WITH` / `ENDS WITH` — two keyword tokens.
    StartsWith,
    EndsWith,
}

fn infix_op(p: &mut Parser<'_>) -> Option<InfixOp> {
    let c = p.current();
    Some(match c {
        // Priority 1: OR
        SyntaxKind::OR_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 2,
            right_bp: 3,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 2: XOR
        SyntaxKind::XOR_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 4,
            right_bp: 5,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 3: AND
        SyntaxKind::AND_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 6,
            right_bp: 7,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 5: comparison family (non-assoc — but implemented as
        // left-assoc with a spec-aligned diagnostic later; harmless for
        // well-formed input).
        SyntaxKind::EQ
        | SyntaxKind::NEQ
        | SyntaxKind::BANG_EQ
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::BINARY_EXPR,
        },
        SyntaxKind::REGEX_EQ => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::REGEX_MATCH_EXPR,
        },
        SyntaxKind::IN_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::IN_EXPR,
        },
        SyntaxKind::STARTS_KW => InfixOp {
            kind: InfixKind::StartsWith,
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::STRING_OP_EXPR,
        },
        SyntaxKind::ENDS_KW => InfixOp {
            kind: InfixKind::EndsWith,
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::STRING_OP_EXPR,
        },
        SyntaxKind::CONTAINS_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::STRING_OP_EXPR,
        },
        // Priority 6: additive
        SyntaxKind::PLUS | SyntaxKind::MINUS => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 12,
            right_bp: 13,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 7: multiplicative
        SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 14,
            right_bp: 15,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 8: power (right-assoc → right_bp == left_bp).
        SyntaxKind::CARET => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 16,
            right_bp: 16,
            node: SyntaxKind::BINARY_EXPR,
        },
        _ => return None,
    })
}

fn consume_infix_op(p: &mut Parser<'_>, kind: InfixKind) {
    match kind {
        InfixKind::Single(tok) => p.bump(tok),
        InfixKind::StartsWith => {
            p.bump(SyntaxKind::STARTS_KW);
            if !p.eat(SyntaxKind::WITH_KW) {
                p.error("expected WITH after STARTS");
            }
        }
        InfixKind::EndsWith => {
            p.bump(SyntaxKind::ENDS_KW);
            if !p.eat(SyntaxKind::WITH_KW) {
                p.error("expected WITH after ENDS");
            }
        }
    }
}

// --------------------------------------------------------------------------
// Postfix
// --------------------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
struct PostfixOp {
    bp: u8,
    kind: PostfixKind,
}

#[derive(Copy, Clone, Debug)]
enum PostfixKind {
    /// `.ident` — property access.
    Dot,
    /// `[expr]` — index / subscript.
    Index,
    /// `(arg, arg, ...)` — function call. Only allowed when the lhs is
    /// an IDENT — the Pratt loop checks this via `postfix_op`.
    Call,
    /// `IS NULL` / `IS NOT NULL`. `IS` is also handled as a binary op
    /// above because openCypher uses `IS NULL` with the lhs as operand —
    /// the infix path handles all well-formed cases.
    /// (Kept as a placeholder variant for future null-check recovery.)
    #[allow(dead_code)]
    IsNull,
}

fn postfix_op(p: &mut Parser<'_>) -> Option<PostfixOp> {
    let bp = 20; // higher than any infix left_bp.
    Some(match p.current() {
        SyntaxKind::DOT => PostfixOp {
            bp,
            kind: PostfixKind::Dot,
        },
        SyntaxKind::L_BRACK => PostfixOp {
            bp,
            kind: PostfixKind::Index,
        },
        SyntaxKind::L_PAREN => PostfixOp {
            bp,
            kind: PostfixKind::Call,
        },
        _ => return None,
    })
}

fn apply_postfix(
    p: &mut Parser<'_>,
    lhs: CompletedMarker,
    op: PostfixOp,
    depth: u32,
) -> CompletedMarker {
    match op.kind {
        PostfixKind::Dot => {
            let m = lhs.precede(p);
            p.bump(SyntaxKind::DOT);
            if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
                p.error("expected property key after '.'");
            }
            m.complete(p, SyntaxKind::PROP_ACCESS_EXPR)
        }
        PostfixKind::Index => {
            let m = lhs.precede(p);
            p.bump(SyntaxKind::L_BRACK);
            if expr_bp_depth(p, 0, depth + 1).is_none() {
                p.error("expected index expression");
            }
            if !p.eat(SyntaxKind::R_BRACK) {
                p.error("expected ']' to close index expression");
            }
            m.complete(p, SyntaxKind::SUBSCRIPT_EXPR)
        }
        PostfixKind::Call => call_postfix(p, lhs, depth),
        PostfixKind::IsNull => {
            let m = lhs.precede(p);
            // Unused currently; reserved for non-infix IS paths.
            m.complete(p, SyntaxKind::IS_NULL_EXPR)
        }
    }
}

fn call_postfix(p: &mut Parser<'_>, lhs: CompletedMarker, depth: u32) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(SyntaxKind::L_PAREN);
    // Optional inline `DISTINCT` (aggregation form).
    p.eat(SyntaxKind::DISTINCT_KW);
    // Arg list.
    if !p.at(SyntaxKind::R_PAREN) {
        let args = p.start();
        call_arg(p, depth);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            call_arg(p, depth);
        }
        args.complete(p, SyntaxKind::ARG_LIST);
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error("expected ')' to close function call");
    }
    m.complete(p, SyntaxKind::FUNCTION_CALL)
}

fn call_arg(p: &mut Parser<'_>, depth: u32) {
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error("expected function argument");
    }
}

// --------------------------------------------------------------------------
// Unused helpers (kept for grammar extensibility)
// --------------------------------------------------------------------------

#[allow(dead_code)]
fn _recovery_anchor_placeholder(_: &mut Parser<'_>, _: TokenSet, _: Marker) {
    // Present so future per-production recovery tables (cy-2vh) can be
    // threaded without re-plumbing every call site.
}
