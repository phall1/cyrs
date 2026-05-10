//! Diagnostic code registry (spec 0001 §10.2).
//!
//! Codes are **stable**. Once assigned, a code's meaning cannot change.
//! New checks get new codes; removed checks leave their code retired,
//! never reused. CI (§17.2 / §17.6) enforces uniqueness.
//!
//! Code ranges:
//!
//! | Range           | Meaning                                  |
//! | --------------- | ---------------------------------------- |
//! | `E0001..=E0999` | Syntax (lexer + parser)                  |
//! | `E1000..=E1999` | Name resolution                          |
//! | `E2000..=E2999` | Semantic — schema-free                   |
//! | `E3000..=E3999` | Semantic — schema-aware                  |
//! | `E4000..=E4999` | Dialect / compatibility                  |
//! | `E5000..=E5999` | Type system                              |
//! | `W6000..=W6999` | Style / lint warnings                    |
//! | `W7000..=W7999` | Performance warnings                     |
//! | `N8000..=N8999` | Informational notes                      |

use core::fmt;

/// Stable diagnostic code. Rendered as `E0001` / `W6001` / `N8001`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(non_camel_case_types)]
pub enum DiagCode {
    // --- syntax (E0001..=E0999) --------------------------------------
    /// Generic / unclassified syntax error.
    ///
    /// Docs: `docs/errors/E0001.md`
    E0001 = 1,
    /// Unexpected token encountered.
    ///
    /// Docs: `docs/errors/E0002.md`
    E0002 = 2,
    /// Expected `<token>`, found `<token>`.
    ///
    /// Docs: `docs/errors/E0003.md`
    E0003 = 3,
    /// Unclosed string literal (missing closing quote).
    ///
    /// Docs: `docs/errors/E0004.md`
    E0004 = 4,
    /// Unclosed block comment (missing `*/`).
    ///
    /// Docs: `docs/errors/E0005.md`
    E0005 = 5,
    /// Invalid numeric literal (bad digits or suffix).
    ///
    /// Docs: `docs/errors/E0006.md`
    E0006 = 6,
    /// Expected a statement (clause keyword) but found something else.
    ///
    /// Docs: `docs/errors/E0007.md`
    E0007 = 7,
    /// Expected `;` or end of input after statement.
    ///
    /// Docs: `docs/errors/E0008.md`
    E0008 = 8,
    /// Expected `(` to start a node pattern.
    ///
    /// Docs: `docs/errors/E0009.md`
    E0009 = 9,
    /// Expected a node pattern after a relationship pattern.
    ///
    /// Docs: `docs/errors/E0010.md`
    E0010 = 10,
    /// Expected `)` to close a node pattern.
    ///
    /// Docs: `docs/errors/E0011.md`
    E0011 = 11,
    /// Expected `-` at the start of a relationship pattern.
    ///
    /// Docs: `docs/errors/E0012.md`
    E0012 = 12,
    /// Expected `-` to close a left-arrow relationship pattern.
    ///
    /// Docs: `docs/errors/E0013.md`
    E0013 = 13,
    /// Expected `-` or `->` to close a relationship pattern.
    ///
    /// Docs: `docs/errors/E0014.md`
    E0014 = 14,
    /// Expected `]` to close a relationship detail block.
    ///
    /// Docs: `docs/errors/E0015.md`
    E0015 = 15,
    /// Expected a label name after `:`.
    ///
    /// Docs: `docs/errors/E0016.md`
    E0016 = 16,
    /// Expected a relationship type name after `:`.
    ///
    /// Docs: `docs/errors/E0017.md`
    E0017 = 17,
    /// Expected `}` to close a property map.
    ///
    /// Docs: `docs/errors/E0018.md`
    E0018 = 18,
    /// Expected a property key identifier.
    ///
    /// Docs: `docs/errors/E0019.md`
    E0019 = 19,
    /// Expected `:` separating a property key from its value.
    ///
    /// Docs: `docs/errors/E0020.md`
    E0020 = 20,
    /// Expected an expression for a property value.
    ///
    /// Docs: `docs/errors/E0021.md`
    E0021 = 21,
    /// Expected an identifier.
    ///
    /// Docs: `docs/errors/E0022.md`
    E0022 = 22,
    /// Expression nesting depth exceeds the parser limit.
    ///
    /// Docs: `docs/errors/E0023.md`
    E0023 = 23,
    /// Expected an operand after a unary operator.
    ///
    /// Docs: `docs/errors/E0024.md`
    E0024 = 24,
    /// Expected `NULL` after `IS` (or `IS NOT`).
    ///
    /// Docs: `docs/errors/E0025.md`
    E0025 = 25,
    /// Expected a right-hand side operand for a binary expression.
    ///
    /// Docs: `docs/errors/E0026.md`
    E0026 = 26,
    /// Expected an expression inside parentheses.
    ///
    /// Docs: `docs/errors/E0027.md`
    E0027 = 27,
    /// Expected `)` to close a parenthesised expression.
    ///
    /// Docs: `docs/errors/E0028.md`
    E0028 = 28,
    /// Expected `WITH` after `STARTS` (i.e. `STARTS WITH`).
    ///
    /// Docs: `docs/errors/E0029.md`
    E0029 = 29,
    /// Expected `WITH` after `ENDS` (i.e. `ENDS WITH`).
    ///
    /// Docs: `docs/errors/E0030.md`
    E0030 = 30,
    /// Expected a property key name after `.`.
    ///
    /// Docs: `docs/errors/E0031.md`
    E0031 = 31,
    /// Expected an index expression inside `[…]`.
    ///
    /// Docs: `docs/errors/E0032.md`
    E0032 = 32,
    /// Expected `]` to close a subscript / index expression.
    ///
    /// Docs: `docs/errors/E0033.md`
    E0033 = 33,
    /// Expected `)` to close a function call argument list.
    ///
    /// Docs: `docs/errors/E0034.md`
    E0034 = 34,
    /// Expected a function call argument expression.
    ///
    /// Docs: `docs/errors/E0035.md`
    E0035 = 35,
    /// Expected an expression in a `RETURN` item.
    ///
    /// Docs: `docs/errors/E0036.md`
    E0036 = 36,
    /// Expected an identifier after `AS` (alias).
    ///
    /// Docs: `docs/errors/E0037.md`
    E0037 = 37,
    /// Expected `BY` after `ORDER` (i.e. `ORDER BY`).
    ///
    /// Docs: `docs/errors/E0038.md`
    E0038 = 38,
    /// Expected an expression in an `ORDER BY` item.
    ///
    /// Docs: `docs/errors/E0039.md`
    E0039 = 39,
    /// Expected an expression after `SKIP`.
    ///
    /// Docs: `docs/errors/E0040.md`
    E0040 = 40,
    /// Expected an expression after `LIMIT`.
    ///
    /// Docs: `docs/errors/E0041.md`
    E0041 = 41,
    /// Expected `MATCH` after `OPTIONAL`.
    ///
    /// Docs: `docs/errors/E0042.md`
    E0042 = 42,
    /// Expected an expression after `WHERE`.
    ///
    /// Docs: `docs/errors/E0043.md`
    E0043 = 43,
    /// Clause keyword encountered that is not yet implemented (deferred construct).
    ///
    /// Docs: `docs/errors/E0044.md`
    E0044 = 44,
    /// Expected a clause keyword (`MATCH`, `WITH`, `RETURN`, …).
    ///
    /// Docs: `docs/errors/E0045.md`
    E0045 = 45,
    /// Invalid escape sequence in a string literal.
    ///
    /// Docs: `docs/errors/E0046.md`
    E0046 = 46,
    /// Expected an expression in a list literal.
    ///
    /// Docs: `docs/errors/E0047.md`
    E0047 = 47,
    /// Expected `]` to close a list literal.
    ///
    /// Docs: `docs/errors/E0048.md`
    E0048 = 48,
    /// Expected a key in a map literal.
    ///
    /// Docs: `docs/errors/E0049.md`
    E0049 = 49,
    /// Expected `:` in a map literal entry.
    ///
    /// Docs: `docs/errors/E0050.md`
    E0050 = 50,
    /// Expected an expression for a map literal value.
    ///
    /// Docs: `docs/errors/E0051.md`
    E0051 = 51,
    /// Expected `}` to close a map literal.
    ///
    /// Docs: `docs/errors/E0052.md`
    E0052 = 52,
    /// Expected an expression after `UNWIND`.
    ///
    /// Docs: `docs/errors/E0053.md`
    E0053 = 53,
    /// Expected `AS` after an `UNWIND` expression.
    ///
    /// Docs: `docs/errors/E0054.md`
    E0054 = 54,
    /// Expected a pattern after `CREATE`.
    ///
    /// Docs: `docs/errors/E0055.md`
    E0055 = 55,
    /// Expected a pattern after `MERGE`.
    ///
    /// Docs: `docs/errors/E0056.md`
    E0056 = 56,
    /// Expected a SET item (property assignment or label add).
    ///
    /// Docs: `docs/errors/E0057.md`
    E0057 = 57,
    /// Expected a REMOVE item (property access or label).
    ///
    /// Docs: `docs/errors/E0058.md`
    E0058 = 58,
    /// Expected an expression after `DELETE`.
    ///
    /// Docs: `docs/errors/E0059.md`
    E0059 = 59,
    /// Expected `DELETE` after `DETACH`.
    ///
    /// Docs: `docs/errors/E0060.md`
    E0060 = 60,
    /// Expected `CREATE` or `MATCH` after `ON` in a MERGE action.
    ///
    /// Docs: `docs/errors/E0061.md`
    E0061 = 61,
    /// Expected `]` to close a variable-length hop quantifier.
    ///
    /// Docs: `docs/errors/E0062.md`
    E0062 = 62,
    /// Expected `=` after a path binder identifier.
    ///
    /// Docs: `docs/errors/E0063.md`
    E0063 = 63,
    /// Expected `(` after a list-predicate keyword (cy-8x5).
    ///
    /// Emitted by the parser when `ANY / ALL / NONE / SINGLE` is not
    /// immediately followed by an opening parenthesis at atom position.
    ///
    /// Docs: `docs/errors/E0065.md`
    E0065 = 65,
    /// Expected `IN` between the list-predicate binder and its source
    /// expression (cy-8x5).
    ///
    /// Docs: `docs/errors/E0066.md`
    E0066 = 66,
    /// Expected `)` to close a list-predicate expression (cy-8x5).
    ///
    /// Docs: `docs/errors/E0067.md`
    E0067 = 67,

    // --- name resolution (E1000–E1999) --------------------------------
    /// Unresolved variable reference (spec §6.2).
    ///
    /// Emitted by `cypher-sema` resolve pass when a name that appears in
    /// an expression position cannot be found in any enclosing scope.
    ///
    /// Docs: `docs/errors/E1001.md`
    E1001 = 1001,
    /// Variable shadows an outer binding (spec §6.2).
    ///
    /// Emitted as a **warning** when `SemaOptions::warn_shadowing` is set
    /// and a new binding introduces a name already visible in an outer scope.
    ///
    /// Docs: `docs/errors/E1002.md`
    E1002 = 1002,

    // --- semantic schema-free (E2000–E2999) --------------------------
    /// Variable used in arithmetic / numeric context but has non-Value kind
    /// (Node, Relationship, or Path). Schema-free kind-consistency check
    /// (spec §6.3).
    ///
    /// Emitted by `cypher-sema` kinds pass when a Node/Relationship/Path
    /// variable appears as an operand of an arithmetic expression.
    ///
    /// Docs: `docs/errors/E2007.md`
    E2007 = 2007,
    /// Variable of incorrect kind used in pattern position (spec §6.3).
    ///
    /// E.g., a `Value` variable where a node-pattern binder is expected, or
    /// a `Node` variable where a path binder is expected.
    ///
    /// Emitted by `cypher-sema` kinds pass.
    ///
    /// Docs: `docs/errors/E2008.md`
    E2008 = 2008,
    /// Arithmetic operand has a non-numeric type (spec §7.4).
    ///
    /// Emitted when an operand of `+`, `-`, `*`, `/`, `%`, or `^` infers to
    /// a type that cannot unify with `Num` (e.g., a boolean or a string).
    ///
    /// Docs: `docs/errors/E2009.md`
    E2009 = 2009,
    /// String-operator operand has a non-string type (spec §7.4).
    ///
    /// Emitted when an operand of `CONTAINS`, `STARTS WITH`, or `ENDS WITH`
    /// infers to a type other than `String` or `Any`.
    ///
    /// Docs: `docs/errors/E2010.md`
    E2010 = 2010,
    /// Boolean-operator operand has a non-boolean type (spec §7.4).
    ///
    /// Emitted when an operand of `AND`, `OR`, `XOR`, or unary `NOT`
    /// infers to a type other than `Bool` or `Any`.
    ///
    /// Docs: `docs/errors/E2011.md`
    E2011 = 2011,
    /// `IN` list element type mismatch (spec §7.4).
    ///
    /// Emitted when the left operand of `IN` infers to a type that cannot
    /// unify with the element type of the right-hand list.
    ///
    /// Docs: `docs/errors/E2012.md`
    E2012 = 2012,
    /// `IN` right-hand operand is not a list (spec §7.4).
    ///
    /// Emitted when the right-hand side of `IN` infers to a non-list type
    /// (e.g., `x IN 42`).
    ///
    /// Docs: `docs/errors/E2013.md`
    E2013 = 2013,

    // --- semantic schema-aware (E3000–E3999) -------------------------
    /// Unknown node label (spec §7.5).
    ///
    /// Emitted by `cypher-sema` schema-aware pass when a label referenced in
    /// a node pattern is not declared in the schema.
    ///
    /// Docs: `docs/errors/E3001.md`
    E3001 = 3001,
    /// Unknown relationship type (spec §7.5).
    ///
    /// Emitted by `cypher-sema` schema-aware pass when a relationship type
    /// referenced in a pattern is not declared in the schema.
    ///
    /// Docs: `docs/errors/E3002.md`
    E3002 = 3002,
    /// Unknown property on label or relationship type (spec §7.5).
    ///
    /// Emitted when a property key in an inline pattern map is not declared
    /// on any of the node's labels or the relationship's type.
    ///
    /// Docs: `docs/errors/E3003.md`
    E3003 = 3003,
    /// Property type mismatch (spec §7.5).
    ///
    /// Emitted when a literal value in an inline property map cannot be
    /// stored in the declared property type (e.g., `String` into an `Int`
    /// slot).
    ///
    /// Docs: `docs/errors/E3004.md`
    E3004 = 3004,
    /// Unknown function name (spec §7.5).
    ///
    /// Emitted by `cypher-sema` schema-aware pass when a function call
    /// references a name not present in the `SchemaProvider`'s catalog.
    ///
    /// Docs: `docs/errors/E3006.md`
    E3006 = 3006,
    /// Function or procedure arity mismatch (spec §7.5).
    ///
    /// Emitted when a function or procedure call provides the wrong number
    /// of arguments compared to the declared signature.
    ///
    /// Docs: `docs/errors/E3007.md`
    E3007 = 3007,
    /// Unknown procedure name (spec §7.5).
    ///
    /// Emitted by `cypher-sema` schema-aware pass when a `CALL` clause
    /// references a procedure not present in the `SchemaProvider`'s catalog.
    ///
    /// Docs: `docs/errors/E3008.md`
    E3008 = 3008,
    /// Schema-file property type is unresolved (spec 0002 §9).
    ///
    /// Emitted by `cypher schema check` when a property or parameter
    /// declares an opaque type (e.g. `DURATION`, `POINT`) whose v0
    /// structural meaning is deferred. Load succeeds — the type round-
    /// trips — but the linter flags it so operators know the type
    /// participates as an opaque symbolic value only.
    ///
    /// Docs: `docs/errors/E3010.md`
    E3010 = 3010,
    /// Relationship-type endpoint cycles through the same label on both
    /// sides (spec 0002 §9).
    ///
    /// Emitted by `cypher schema check` when a rel type declares the
    /// same label in both `start_labels` and `end_labels`. The spec
    /// permits self-loops but the linter surfaces them so operators
    /// can confirm they are intentional (e.g. `KNOWS: Person → Person`
    /// is fine; `REPORTS_TO: Team → Team` may be a modelling slip).
    ///
    /// Docs: `docs/errors/E3011.md`
    E3011 = 3011,

    // --- dialect (E4000–E4999) ----------------------------------------
    /// Dialect not supported (spec §9.3).
    ///
    /// Emitted by `cypher-sema::dialect::reject_neo4j_current` when the
    /// caller attempts to use the `Neo4jCurrent` dialect, which is not
    /// part of v1 (spec §9.3, §19–§20).
    ///
    /// Docs: `docs/errors/E4001.md`
    E4001 = 4001,

    // --- dialect gates (E4010–E4019) assigned by cy-z49 --------------
    /// `label_negation` — `!` in label expressions; `GqlAligned` only (spec §9.2).
    ///
    /// Docs: `docs/errors/E4010.md`
    E4010 = 4010,
    /// `label_union` — `A|B` label-union in patterns; allowed in both v1 dialects (spec §9.2).
    ///
    /// Docs: `docs/errors/E4011.md`
    E4011 = 4011,
    /// `integer_division` — `/` with integer operands; `OpenCypherV9` only (spec §9.2).
    ///
    /// Docs: `docs/errors/E4012.md`
    E4012 = 4012,
    /// `union_set` — plain `UNION` (set-semantics); allowed in both v1 dialects.
    ///
    /// Docs: `docs/errors/E4013.md`
    E4013 = 4013,
    /// `call_procedure` — `CALL proc YIELD …`; allowed in both v1 dialects.
    ///
    /// Docs: `docs/errors/E4014.md`
    E4014 = 4014,
    /// `load_csv` — `LOAD CSV` clause; deferred (spec §9.3).
    ///
    /// Docs: `docs/errors/E4015.md`
    E4015 = 4015,
    /// `apoc_functions` — APOC-prefixed functions; deferred (spec §9.3).
    ///
    /// Docs: `docs/errors/E4016.md`
    E4016 = 4016,
    /// `exists_subquery` — `EXISTS { }` subquery; deferred (spec §9.3).
    ///
    /// Docs: `docs/errors/E4017.md`
    E4017 = 4017,
    /// `cypher_prefix` — `CYPHER n MATCH …` header; deferred (spec §9.3).
    ///
    /// Docs: `docs/errors/E4018.md`
    E4018 = 4018,
    /// `call_in_transactions` — `CALL { } IN TRANSACTIONS`; deferred (spec §9.3).
    ///
    /// Docs: `docs/errors/E4019.md`
    E4019 = 4019,

    // --- type system (E5000–E5999) ------------------------------------
    /// Type mismatch in unification — two incompatible concrete types cannot
    /// be unified (spec §7.2, §7.3).
    ///
    /// Produced by `cypher-sema::unify::TypeMismatch::into_diagnostic`.
    /// Call sites supply the source range; code is always `E5003`.
    ///
    /// Docs: `docs/errors/E5003.md`
    E5003 = 5003,
    /// Indexing or slicing applied to a non-list expression (spec §7.4 /
    /// §19 row "List indexing / slicing").
    ///
    /// Emitted by `cypher-sema::kinds` when `xs[0]` / `xs[i..j]` is applied
    /// to a target whose inferred type is not a list (and not `Any` / `Unknown`).
    /// v1 scope is list-only; string indexing is deferred to a follow-up
    /// bead and is rejected by this code until then.
    ///
    /// Docs: `docs/errors/E5010.md`
    E5010 = 5010,
    /// List-predicate iterable is not a list (spec §7.4 / §19 row
    /// "List predicates").
    ///
    /// Emitted by `cypher-sema::kinds` when the source expression of
    /// `ANY / ALL / NONE / SINGLE (x IN xs [WHERE p])` is structurally
    /// non-list: a scalar literal, a `Node` / `Relationship` / `Path`
    /// variable, a map literal, or a pattern predicate. `Value` variables
    /// and call results are accepted because they may hold a list at
    /// runtime — schema-aware refinement is the follow-up's job.
    ///
    /// Docs: `docs/errors/E5011.md`
    E5011 = 5011,
    /// Built-in function argument kind mismatch (spec §7.4 / §10.2).
    ///
    /// Emitted by `cypher-sema::infer` when a call to a registered
    /// builtin passes an argument whose inferred type is incompatible
    /// with the parameter's declared `ArgShape`. The canonical case
    /// is `id(42)` — the `id` builtin's parameter is `GraphEntity`
    /// (`Node | Relationship | Path`), so a scalar literal is rejected.
    ///
    /// Docs: `docs/errors/E5012.md`
    E5012 = 5012,

    // --- syntax (E0064) continued -------------------------------------
    /// Unclosed `[` in a list-indexing / slicing expression (spec §4.3).
    ///
    /// Emitted by the parser's `index_or_slice_postfix` recovery path
    /// when no matching `]` follows the inner expression / slice bounds.
    /// Distinct from E0033 (`SUBSCRIPT_EXPR` legacy path) so tooling can
    /// tell the two apart; the typed cy-7s6.1 grammar path uses this
    /// code exclusively.
    ///
    /// Docs: `docs/errors/E0064.md`
    E0064 = 64,
    /// Expected `IN` in list comprehension (spec §19 row
    /// "List comprehensions").
    ///
    /// Emitted by the parser's list-comprehension production (cy-5gh)
    /// when the iteration-variable identifier is not followed by the
    /// `IN` keyword.
    ///
    /// Docs: `docs/errors/E0068.md`
    E0068 = 68,
    /// Expected `|` or `]` in list comprehension (spec §19 row
    /// "List comprehensions").
    ///
    /// Emitted by the parser's list-comprehension production (cy-5gh)
    /// when neither a projection pipe `|` nor the closing bracket `]`
    /// follows the optional `WHERE` predicate / source expression.
    ///
    /// Docs: `docs/errors/E0069.md`
    E0069 = 69,
    /// Expected `THEN` in a `CASE` arm (spec §19 row "CASE").
    ///
    /// Emitted by the parser's CASE production (cy-41u) when a
    /// `WHEN <value>` clause is not followed by the mandatory `THEN`
    /// keyword.
    ///
    /// Docs: `docs/errors/E0070.md`
    E0070 = 70,
    /// Expected `END` to close a `CASE` expression (spec §19 row "CASE").
    ///
    /// Emitted by the parser's CASE production (cy-41u) when the
    /// terminating `END` keyword is missing after the final arm /
    /// optional `ELSE` branch.
    ///
    /// Docs: `docs/errors/E0071.md`
    E0071 = 71,
    /// Expected `)` to close an `EXISTS(<pattern>)` pattern predicate
    /// (spec §6.1 / §19 row "Pattern predicates in expressions").
    ///
    /// Emitted by the parser's `exists_pattern_predicate` production
    /// (cy-lve) when the closing paren of the wrapping
    /// `EXISTS ( <pattern> )` form is missing. The bare-pattern form
    /// `WHERE (a)-->(b)` (cy-7lf) does not emit this code — the inner
    /// `)` belongs to the pattern's node-pattern production.
    ///
    /// Docs: `docs/errors/E0072.md`
    E0072 = 72,
    /// Expected `}` to close a map projection (spec §6.1 / §19 row
    /// "Map projection"; cy-01q).
    ///
    /// Emitted by the parser's `map_projection_postfix` production
    /// when the closing `}` of `n { ... }` is missing. Distinct from
    /// E0052 (map literal `}`) so tools can distinguish the two
    /// recovery paths.
    E0078 = 78,
    /// Expected property name or `*` after `.` in a map projection item
    /// (cy-01q). Inside `n { ... }`, a `.` opens either `.NAME`
    /// (property selector) or `.*` (all-properties spread); anything
    /// else is an error.
    E0079 = 79,
    /// Expected `:` in a map projection literal item (cy-01q).
    /// Distinct from E0050 (map literal `:`).
    E0080 = 80,
    /// Expected expression for map projection literal value (cy-01q).
    E0081 = 81,
    /// Expected an item in map projection: `.name`, `key: expr`, `.*`,
    /// or `*` (cy-01q).
    E0082 = 82,

    // --- warnings (6000..) -------------------------------------------
    /// Dead WITH — projection with no downstream reader.
    W6001 = 6001,
    /// Identifier collides with a reserved keyword; needs backtick quoting.
    W6002 = 6002,
    /// Duplicate key in map literal — last write wins.
    W6003 = 6003,
    /// Variable bound but never read.
    W6004 = 6004,
    /// Redundant OPTIONAL MATCH — no bound variables escape the clause.
    W6005 = 6005,
    /// Pattern has no label or type restriction — will scan broadly.
    W6006 = 6006,
    /// Inconsistent keyword casing inside one query.
    W6007 = 6007,
    /// Unreachable label — schema-file lint (spec 0002 §9).
    ///
    /// Emitted by `cypher schema check` when a label is declared but
    /// not referenced by any relationship type's `start_labels` or
    /// `end_labels`. Not fatal: isolated labels are permitted — they
    /// are legitimate for nodes created without relationships — but
    /// the lint surfaces them so operators catch typos in rel-type
    /// endpoint lists.
    ///
    /// Docs: `docs/errors/W6010.md`
    W6010 = 6010,

    // --- performance (7000..) ----------------------------------------
    /// Cartesian product between disconnected MATCH components.
    W7001 = 7001,
    /// Expensive function call inside a row-wise filter.
    W7002 = 7002,
    /// Variable-length path without an upper bound.
    W7003 = 7003,
    /// Property access on an unindexed label in a selective filter.
    W7004 = 7004,

    // --- notes (8000..) ----------------------------------------------
    /// Informational — pattern normalised to canonical direction.
    N8001 = 8001,
    /// Informational — inferred type of an expression.
    N8002 = 8002,
    /// Informational — variable dropped from scope by this projection.
    N8003 = 8003,
}

impl DiagCode {
    /// Render as the stable wire-format string: `E0001`, `W6001`, `N8001`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E0001 => "E0001",
            Self::E0002 => "E0002",
            Self::E0003 => "E0003",
            Self::E0004 => "E0004",
            Self::E0005 => "E0005",
            Self::E0006 => "E0006",
            Self::E0007 => "E0007",
            Self::E0008 => "E0008",
            Self::E0009 => "E0009",
            Self::E0010 => "E0010",
            Self::E0011 => "E0011",
            Self::E0012 => "E0012",
            Self::E0013 => "E0013",
            Self::E0014 => "E0014",
            Self::E0015 => "E0015",
            Self::E0016 => "E0016",
            Self::E0017 => "E0017",
            Self::E0018 => "E0018",
            Self::E0019 => "E0019",
            Self::E0020 => "E0020",
            Self::E0021 => "E0021",
            Self::E0022 => "E0022",
            Self::E0023 => "E0023",
            Self::E0024 => "E0024",
            Self::E0025 => "E0025",
            Self::E0026 => "E0026",
            Self::E0027 => "E0027",
            Self::E0028 => "E0028",
            Self::E0029 => "E0029",
            Self::E0030 => "E0030",
            Self::E0031 => "E0031",
            Self::E0032 => "E0032",
            Self::E0033 => "E0033",
            Self::E0034 => "E0034",
            Self::E0035 => "E0035",
            Self::E0036 => "E0036",
            Self::E0037 => "E0037",
            Self::E0038 => "E0038",
            Self::E0039 => "E0039",
            Self::E0040 => "E0040",
            Self::E0041 => "E0041",
            Self::E0042 => "E0042",
            Self::E0043 => "E0043",
            Self::E0044 => "E0044",
            Self::E0045 => "E0045",
            Self::E0046 => "E0046",
            Self::E0047 => "E0047",
            Self::E0048 => "E0048",
            Self::E0049 => "E0049",
            Self::E0050 => "E0050",
            Self::E0051 => "E0051",
            Self::E0052 => "E0052",
            Self::E0053 => "E0053",
            Self::E0054 => "E0054",
            Self::E0055 => "E0055",
            Self::E0056 => "E0056",
            Self::E0057 => "E0057",
            Self::E0058 => "E0058",
            Self::E0059 => "E0059",
            Self::E0060 => "E0060",
            Self::E0061 => "E0061",
            Self::E0062 => "E0062",
            Self::E0063 => "E0063",
            Self::E0064 => "E0064",
            Self::E0065 => "E0065",
            Self::E0066 => "E0066",
            Self::E0067 => "E0067",
            Self::E0068 => "E0068",
            Self::E0069 => "E0069",
            Self::E0070 => "E0070",
            Self::E0071 => "E0071",
            Self::E0072 => "E0072",
            Self::E0078 => "E0078",
            Self::E0079 => "E0079",
            Self::E0080 => "E0080",
            Self::E0081 => "E0081",
            Self::E0082 => "E0082",
            Self::E1001 => "E1001",
            Self::E1002 => "E1002",
            Self::E2007 => "E2007",
            Self::E2008 => "E2008",
            Self::E2009 => "E2009",
            Self::E2010 => "E2010",
            Self::E2011 => "E2011",
            Self::E2012 => "E2012",
            Self::E2013 => "E2013",
            Self::E3001 => "E3001",
            Self::E3002 => "E3002",
            Self::E3003 => "E3003",
            Self::E3004 => "E3004",
            Self::E3006 => "E3006",
            Self::E3007 => "E3007",
            Self::E3008 => "E3008",
            Self::E3010 => "E3010",
            Self::E3011 => "E3011",
            Self::E4001 => "E4001",
            Self::E4010 => "E4010",
            Self::E4011 => "E4011",
            Self::E4012 => "E4012",
            Self::E4013 => "E4013",
            Self::E4014 => "E4014",
            Self::E4015 => "E4015",
            Self::E4016 => "E4016",
            Self::E4017 => "E4017",
            Self::E4018 => "E4018",
            Self::E4019 => "E4019",
            Self::E5003 => "E5003",
            Self::E5010 => "E5010",
            Self::E5011 => "E5011",
            Self::E5012 => "E5012",
            Self::W6001 => "W6001",
            Self::W6002 => "W6002",
            Self::W6003 => "W6003",
            Self::W6004 => "W6004",
            Self::W6005 => "W6005",
            Self::W6006 => "W6006",
            Self::W6007 => "W6007",
            Self::W6010 => "W6010",
            Self::W7001 => "W7001",
            Self::W7002 => "W7002",
            Self::W7003 => "W7003",
            Self::W7004 => "W7004",
            Self::N8001 => "N8001",
            Self::N8002 => "N8002",
            Self::N8003 => "N8003",
        }
    }

    /// Severity letter derived from the numeric range (spec §10.2).
    ///
    /// `0..=5999` → `'E'`, `6000..=7999` → `'W'`, `8000..=8999` → `'N'`.
    /// Panics if the discriminant falls outside any registered range —
    /// the [`ALL`](Self::ALL) invariants enforced by `tests/registry.rs`
    /// make this unreachable at runtime.
    #[must_use]
    pub const fn severity_char(self) -> char {
        match self as u32 {
            0..=5999 => 'E',
            6000..=7999 => 'W',
            8000..=8999 => 'N',
            _ => panic!("DiagCode discriminant outside any registered range"),
        }
    }

    /// Canonical enumeration of every registered diagnostic code, in
    /// numeric order. This is THE registry used by
    /// `tests/registry.rs` to enforce spec §10.2 invariants — every
    /// variant added to [`DiagCode`] must also be appended here.
    pub const ALL: &'static [DiagCode] = &[
        Self::E0001,
        Self::E0002,
        Self::E0003,
        Self::E0004,
        Self::E0005,
        Self::E0006,
        Self::E0007,
        Self::E0008,
        Self::E0009,
        Self::E0010,
        Self::E0011,
        Self::E0012,
        Self::E0013,
        Self::E0014,
        Self::E0015,
        Self::E0016,
        Self::E0017,
        Self::E0018,
        Self::E0019,
        Self::E0020,
        Self::E0021,
        Self::E0022,
        Self::E0023,
        Self::E0024,
        Self::E0025,
        Self::E0026,
        Self::E0027,
        Self::E0028,
        Self::E0029,
        Self::E0030,
        Self::E0031,
        Self::E0032,
        Self::E0033,
        Self::E0034,
        Self::E0035,
        Self::E0036,
        Self::E0037,
        Self::E0038,
        Self::E0039,
        Self::E0040,
        Self::E0041,
        Self::E0042,
        Self::E0043,
        Self::E0044,
        Self::E0045,
        Self::E0046,
        Self::E0047,
        Self::E0048,
        Self::E0049,
        Self::E0050,
        Self::E0051,
        Self::E0052,
        Self::E0053,
        Self::E0054,
        Self::E0055,
        Self::E0056,
        Self::E0057,
        Self::E0058,
        Self::E0059,
        Self::E0060,
        Self::E0061,
        Self::E0062,
        Self::E0063,
        Self::E0064,
        Self::E0065,
        Self::E0066,
        Self::E0067,
        Self::E0068,
        Self::E0069,
        Self::E0070,
        Self::E0071,
        Self::E0072,
        Self::E0078,
        Self::E0079,
        Self::E0080,
        Self::E0081,
        Self::E0082,
        Self::E1001,
        Self::E1002,
        Self::E2007,
        Self::E2008,
        Self::E2009,
        Self::E2010,
        Self::E2011,
        Self::E2012,
        Self::E2013,
        Self::E3001,
        Self::E3002,
        Self::E3003,
        Self::E3004,
        Self::E3006,
        Self::E3007,
        Self::E3008,
        Self::E3010,
        Self::E3011,
        Self::E4001,
        Self::E4010,
        Self::E4011,
        Self::E4012,
        Self::E4013,
        Self::E4014,
        Self::E4015,
        Self::E4016,
        Self::E4017,
        Self::E4018,
        Self::E4019,
        Self::E5003,
        Self::E5010,
        Self::E5011,
        Self::E5012,
        Self::W6001,
        Self::W6002,
        Self::W6003,
        Self::W6004,
        Self::W6005,
        Self::W6006,
        Self::W6007,
        Self::W6010,
        Self::W7001,
        Self::W7002,
        Self::W7003,
        Self::W7004,
        Self::N8001,
        Self::N8002,
        Self::N8003,
    ];
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::DiagCode;

    /// Every code rendered as a string has a unique textual form — the
    /// canonical registry uniqueness check (spec §10.2).
    #[test]
    fn codes_are_unique_strings() {
        let all = [
            DiagCode::E0001,
            DiagCode::E0002,
            DiagCode::E0003,
            DiagCode::E0004,
            DiagCode::E0005,
            DiagCode::E0006,
            DiagCode::E0007,
            DiagCode::E0008,
            DiagCode::E0009,
            DiagCode::E0010,
            DiagCode::E0011,
            DiagCode::E0012,
            DiagCode::E0013,
            DiagCode::E0014,
            DiagCode::E0015,
            DiagCode::E0016,
            DiagCode::E0017,
            DiagCode::E0018,
            DiagCode::E0019,
            DiagCode::E0020,
            DiagCode::E0021,
            DiagCode::E0022,
            DiagCode::E0023,
            DiagCode::E0024,
            DiagCode::E0025,
            DiagCode::E0026,
            DiagCode::E0027,
            DiagCode::E0028,
            DiagCode::E0029,
            DiagCode::E0030,
            DiagCode::E0031,
            DiagCode::E0032,
            DiagCode::E0033,
            DiagCode::E0034,
            DiagCode::E0035,
            DiagCode::E0036,
            DiagCode::E0037,
            DiagCode::E0038,
            DiagCode::E0039,
            DiagCode::E0040,
            DiagCode::E0041,
            DiagCode::E0042,
            DiagCode::E0043,
            DiagCode::E0044,
            DiagCode::E0045,
            DiagCode::E0046,
            DiagCode::E0047,
            DiagCode::E0048,
            DiagCode::E0049,
            DiagCode::E0050,
            DiagCode::E0051,
            DiagCode::E0052,
            DiagCode::E0053,
            DiagCode::E0054,
            DiagCode::E0055,
            DiagCode::E0056,
            DiagCode::E0057,
            DiagCode::E0058,
            DiagCode::E0059,
            DiagCode::E0060,
            DiagCode::E0061,
            DiagCode::E0062,
            DiagCode::E0063,
            DiagCode::E0064,
            DiagCode::E0065,
            DiagCode::E0066,
            DiagCode::E0067,
            DiagCode::E0068,
            DiagCode::E0069,
            DiagCode::E0070,
            DiagCode::E0071,
            DiagCode::E0072,
            DiagCode::E0078,
            DiagCode::E0079,
            DiagCode::E0080,
            DiagCode::E0081,
            DiagCode::E0082,
            DiagCode::E1001,
            DiagCode::E1002,
            DiagCode::E2007,
            DiagCode::E2008,
            DiagCode::E2009,
            DiagCode::E2010,
            DiagCode::E2011,
            DiagCode::E2012,
            DiagCode::E2013,
            DiagCode::E3001,
            DiagCode::E3002,
            DiagCode::E3003,
            DiagCode::E3004,
            DiagCode::E3006,
            DiagCode::E3007,
            DiagCode::E3008,
            DiagCode::E3010,
            DiagCode::E3011,
            DiagCode::E4001,
            DiagCode::E4010,
            DiagCode::E4011,
            DiagCode::E4012,
            DiagCode::E4013,
            DiagCode::E4014,
            DiagCode::E4015,
            DiagCode::E4016,
            DiagCode::E4017,
            DiagCode::E4018,
            DiagCode::E4019,
            DiagCode::E5003,
            DiagCode::E5010,
            DiagCode::E5011,
            DiagCode::E5012,
            DiagCode::W6001,
            DiagCode::W6002,
            DiagCode::W6003,
            DiagCode::W6004,
            DiagCode::W6005,
            DiagCode::W6006,
            DiagCode::W6007,
            DiagCode::W6010,
            DiagCode::W7001,
            DiagCode::W7002,
            DiagCode::W7003,
            DiagCode::W7004,
            DiagCode::N8001,
            DiagCode::N8002,
            DiagCode::N8003,
        ];
        let mut strs: Vec<_> = all.iter().map(|c| c.as_str()).collect();
        strs.sort_unstable();
        strs.dedup();
        assert_eq!(strs.len(), all.len(), "duplicate DiagCode string");
    }
}
