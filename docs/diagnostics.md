# Diagnostic codes

Every diagnostic emitted by `cyrs` carries a stable code of the form
`E0001` / `W6001` / `N8001`. This page indexes every registered code so
contributors, LSP clients, and CI scripts can resolve any code to its
meaning without grepping the compiler.

The authoritative registry is
[`crates/cypher-diag/src/codes.rs`](../crates/cypher-diag/src/codes.rs).
Every emit site references a registered constant; raw strings are
forbidden by CI lint (AGENTS.md §7).

## Ranges

| Range           | Meaning                                   |
| --------------- | ----------------------------------------- |
| `E0001–E0999`   | Syntax (lexer + parser)                   |
| `E1000–E1999`   | Name resolution                           |
| `E2000–E2999`   | Semantic — schema-free                    |
| `E3000–E3999`   | Semantic — schema-aware                   |
| `E4000–E4999`   | Dialect / compatibility                   |
| `E5000–E5999`   | Type system                               |
| `W6000–W6999`   | Style / lint warnings                     |
| `W7000–W7999`   | Performance warnings                     |
| `N8000–N8999`   | Informational notes                       |

Codes are SemVer-stable. Once assigned, a code's meaning never changes.
Retired codes are never reused (AGENTS.md §7). The count is pinned at
**120** in `crates/cypher-diag/tests/registry.rs`.

## E0001–E0999 — Syntax (lexer + parser)

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| [E0001](#e0001) | generic-syntax-error | Generic / unclassified syntax error. | — |
| E0002 | unexpected-token | Emitted when an unexpected token is encountered. | — |
| E0003 | expected-token | Emitted when a specific token was expected but another was found. | — |
| E0004 | unclosed-string-literal | Emitted when a string literal is missing its closing quote. | — |
| E0005 | unclosed-block-comment | Emitted when a block comment is missing its terminating `*/`. | — |
| E0006 | invalid-numeric-literal | Emitted for numeric literals with bad digits or suffix. | — |
| E0007 | expected-statement | Emitted when a clause keyword was expected but something else was found. | — |
| E0008 | expected-semicolon-or-eof | Emitted when `;` or end of input was expected after a statement. | `MATCH p (a)-[]->(b) RETURN p` |
| E0009 | expected-l-paren-node | Emitted when `(` was expected to start a node pattern. | `MATCH p (a)-[]->(b) RETURN p` |
| E0010 | expected-node-after-rel | Emitted when a node pattern was expected after a relationship pattern. | — |
| [E0011](#e0011) | expected-r-paren-node | Emitted when `)` was expected to close a node pattern. | — |
| E0012 | expected-dash-rel-start | Emitted when `-` was expected at the start of a relationship pattern. | — |
| E0013 | expected-dash-left-arrow | Emitted when `-` was expected to close a left-arrow relationship pattern. | — |
| E0014 | expected-rel-close | Emitted when `-` or `->` was expected to close a relationship pattern. | — |
| E0015 | expected-r-bracket-rel-detail | Emitted when `]` was expected to close a relationship detail block. | — |
| E0016 | expected-label-name | Emitted when a label name was expected after `:`. | — |
| E0017 | expected-rel-type-name | Emitted when a relationship type name was expected after `:`. | — |
| E0018 | expected-r-brace-pattern-map | Emitted when `}` was expected to close a property map. | — |
| E0019 | expected-property-key | Emitted when a property key identifier was expected. | — |
| E0020 | expected-colon-property | Emitted when `:` was expected separating a property key from its value. | — |
| E0021 | expected-property-value | Emitted when an expression was expected for a property value. | — |
| E0022 | expected-identifier | Emitted when an identifier was expected. | — |
| E0023 | expression-nesting-depth | Emitted when expression nesting depth exceeds the parser limit. | — |
| E0024 | expected-unary-operand | Emitted when an operand was expected after a unary operator. | — |
| E0025 | expected-null-after-is | Emitted when `NULL` was expected after `IS` (or `IS NOT`). | — |
| E0026 | expected-binary-rhs | Emitted when a right-hand operand was expected for a binary expression. | — |
| E0027 | expected-paren-expression | Emitted when an expression was expected inside parentheses. | — |
| E0028 | expected-r-paren-expression | Emitted when `)` was expected to close a parenthesised expression. | — |
| E0029 | expected-with-after-starts | Emitted when `WITH` was expected after `STARTS` (i.e. `STARTS WITH`). | — |
| E0030 | expected-with-after-ends | Emitted when `WITH` was expected after `ENDS` (i.e. `ENDS WITH`). | — |
| E0031 | expected-property-key-after-dot | Emitted when a property key name was expected after `.`. | — |
| E0032 | expected-index-expression | Emitted when an index expression was expected inside `[…]`. | — |
| E0033 | expected-r-bracket-subscript | Emitted when `]` was expected to close a subscript / index expression. | — |
| E0034 | expected-r-paren-call-args | Emitted when `)` was expected to close a function call argument list. | — |
| E0035 | expected-call-argument | Emitted when a function call argument expression was expected. | — |
| E0036 | expected-return-item | Emitted when an expression was expected in a `RETURN` item. | `MATCH (n) RETURN` |
| E0037 | expected-alias-identifier | Emitted when an identifier was expected after `AS` (alias). | — |
| E0038 | expected-by-after-order | Emitted when `BY` was expected after `ORDER` (i.e. `ORDER BY`). | — |
| E0039 | expected-order-by-item | Emitted when an expression was expected in an `ORDER BY` item. | — |
| E0040 | expected-skip-expression | Emitted when an expression was expected after `SKIP`. | — |
| E0041 | expected-limit-expression | Emitted when an expression was expected after `LIMIT`. | — |
| E0042 | expected-match-after-optional | Emitted when `MATCH` was expected after `OPTIONAL`. | — |
| E0043 | expected-where-expression | Emitted when an expression was expected after `WHERE`. | — |
| [E0044](#e0044) | clause-not-implemented | Raised when a clause keyword is recognised but not yet implemented. | — |
| E0045 | expected-clause-keyword | Emitted when a clause keyword (`MATCH`, `WITH`, `RETURN`, …) was expected. | — |
| E0046 | invalid-string-escape | Emitted for an invalid escape sequence in a string literal. | — |
| E0047 | expected-list-element | Emitted when an expression was expected in a list literal. | `RETURN [,1]` |
| E0048 | expected-r-bracket-list | Emitted when `]` was expected to close a list literal. | `RETURN [1, 2, 3` |
| E0049 | expected-map-key | Emitted when a key was expected in a map literal. | `RETURN {:1}` |
| E0050 | expected-colon-map-entry | Emitted when `:` was expected in a map literal entry. | `RETURN {a 1}` |
| E0051 | expected-map-value | Emitted when an expression was expected for a map literal value. | `RETURN {a:}` |
| E0052 | expected-r-brace-map | Emitted when `}` was expected to close a map literal. | `RETURN {a:1` |
| E0053 | expected-unwind-expression | Emitted when an expression was expected after `UNWIND`. | `UNWIND AS x RETURN x` |
| E0054 | expected-as-after-unwind | Emitted when `AS` was expected after an `UNWIND` expression. | `UNWIND [1, 2, 3] x RETURN x` |
| E0055 | expected-create-pattern | Emitted when a pattern was expected after `CREATE`. | `CREATE RETURN 1` |
| E0056 | expected-merge-pattern | Emitted when a pattern was expected after `MERGE`. | `MERGE RETURN 1` |
| E0057 | expected-set-item | Emitted when a SET item (property assignment or label add) was expected. | `MATCH (n) SET` |
| E0058 | expected-remove-item | Emitted when a REMOVE item (property access or label) was expected. | `MATCH (n) REMOVE RETURN 1` |
| E0059 | expected-delete-expression | Emitted when an expression was expected after `DELETE`. | `MATCH (n) DELETE` |
| E0060 | expected-delete-after-detach | Emitted when `DELETE` was expected after `DETACH`. | `MATCH (n) DETACH n` |
| E0061 | expected-merge-on-action | Emitted when `CREATE` or `MATCH` was expected after `ON` in a MERGE action. | `MERGE (n) ON RETURN 1` |
| E0062 | expected-r-bracket-hop-quantifier | Emitted when `]` was expected to close a variable-length hop quantifier. | — |
| E0063 | expected-eq-path-binder | Emitted when `=` was expected after a path binder identifier. | — |
| [E0064](#e0064) | unclosed-index-bracket | Emitted when `[` in a list-indexing / slicing expression is not matched by `]`. | — |
| E0065 | expected-l-paren-list-predicate | Emitted when `(` was expected after a list-predicate keyword (`ANY`/`ALL`/`NONE`/`SINGLE`). | — |
| E0066 | expected-in-list-predicate | Emitted when `IN` was expected between the list-predicate binder and its source expression. | — |
| E0067 | expected-r-paren-list-predicate | Emitted when `)` was expected to close a list-predicate expression. | — |
| E0068 | expected-in-list-comp | Emitted when `IN` was expected in a list comprehension. | — |
| E0069 | expected-pipe-or-r-bracket-list-comp | Emitted when `\|` or `]` was expected in a list comprehension. | — |
| E0070 | expected-then-case-arm | Emitted when `THEN` was expected in a `CASE` arm. | `RETURN CASE WHEN 1 = 1 'yes' ELSE 'no' END` |
| E0071 | expected-end-case | Emitted when `END` was expected to close a `CASE` expression. | `RETURN CASE WHEN 1 = 1 THEN 'yes' ELSE 'no'` |
| E0072 | expected-r-paren-exists-pattern | Emitted when `)` was expected to close an `EXISTS(<pattern>)` pattern predicate. | `RETURN EXISTS((a)-->(b)` |

## E1000–E1999 — Name resolution

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| E1001 | unresolved-variable | Emitted when a name in expression position cannot be found in any enclosing scope. | `MATCH (n) RETURN z` |
| E1002 | shadowed-variable | Warning raised when a new binding shadows an outer binding and `warn_shadowing` is set. | — |

## E2000–E2999 — Semantic, schema-free

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| E2007 | node-rel-in-arithmetic | Emitted when a Node / Relationship / Path variable appears as an arithmetic operand. | `MATCH (n) RETURN n + 1` |
| E2008 | wrong-kind-in-pattern | Emitted when a variable of the wrong kind appears in pattern position. | — |
| E2009 | arithmetic-non-numeric | Emitted when an arithmetic operand's type cannot unify with `Num`. | `RETURN "hello" + 1` |
| E2010 | string-op-non-string | Emitted when `CONTAINS` / `STARTS WITH` / `ENDS WITH` receives a non-string operand. | `RETURN 42 CONTAINS "x"` |
| E2011 | bool-op-non-bool | Emitted when `AND` / `OR` / `XOR` / `NOT` receives a non-boolean operand. | `RETURN 1 AND 2` |
| E2012 | in-element-type-mismatch | Emitted when the left operand of `IN` cannot unify with the right-hand list's element type. | — |
| E2013 | in-rhs-not-list | Emitted when the right-hand operand of `IN` is not a list. | `RETURN 1 IN 42` |

## E3000–E3999 — Semantic, schema-aware

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| E3001 | unknown-label | Emitted when a node-pattern label is not declared in the schema. | `MATCH (n:Ghost) RETURN n` |
| E3002 | unknown-rel-type | Emitted when a relationship-pattern type is not declared in the schema. | `MATCH (a)-[r:FOLLOWS]->(b) RETURN r` |
| E3003 | unknown-property | Emitted when an inline property key is not declared on the relevant label or rel type. | — |
| E3004 | property-type-mismatch | Emitted when a literal value cannot be stored in the declared property type. | — |
| E3006 | unknown-function | Emitted when a function call references a name not in the `SchemaProvider` catalog. | `RETURN nonexistent_fn(1, 2)` |
| E3007 | arity-mismatch | Emitted when a function or procedure call has the wrong number of arguments. | `RETURN my_func(1, 2, 3)` |
| E3008 | unknown-procedure | Emitted when a `CALL` references a procedure not in the `SchemaProvider` catalog. | `CALL unknown.procedure() YIELD x RETURN x` |
| [E3010](#e3010) | opaque-schema-type | Emitted by `cypher schema check` when a property or parameter declares an opaque v0 type. | — |
| [E3011](#e3011) | self-referential-rel-type | Emitted by `cypher schema check` when a rel type's start and end label lists overlap. | — |

## E4000–E4999 — Dialect / compatibility

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| E4001 | dialect-not-supported | Emitted when the caller selects the `Neo4jCurrent` dialect, which is not part of v1. | — |
| E4010 | gate-label-negation | Gate: `!` in label expressions — permitted only in `GqlAligned`. | — |
| E4011 | gate-label-union | Gate: `A\|B` label-union in patterns — permitted in both v1 dialects. | — |
| E4012 | gate-integer-division | Gate: `/` with integer operands — permitted only in `OpenCypherV9`. | — |
| E4013 | gate-union-set | Gate: plain `UNION` (set-semantics) — permitted in both v1 dialects. | — |
| E4014 | gate-call-procedure | Gate: `CALL proc YIELD …` — permitted in both v1 dialects. | — |
| E4015 | gate-load-csv | Gate: `LOAD CSV` clause — deferred. | — |
| E4016 | gate-apoc-functions | Gate: APOC-prefixed functions / procedures — deferred. | — |
| E4017 | gate-exists-subquery | Gate: `EXISTS { }` subquery — deferred. | — |
| E4018 | gate-cypher-prefix | Gate: `CYPHER n MATCH …` header — deferred. | — |
| E4019 | gate-call-in-transactions | Gate: `CALL { } IN TRANSACTIONS` — deferred. | — |

## E5000–E5999 — Type system

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| [E5003](#e5003) | type-mismatch | Emitted when two concrete types cannot be unified. | — |
| E5010 | index-non-list | Emitted when `xs[i]` or `xs[i..j]` is applied to a non-list target. | `MATCH (n) RETURN n[0]` |
| E5011 | list-predicate-non-list | Emitted when `ANY`/`ALL`/`NONE`/`SINGLE` receives a structurally non-list iterable. | `MATCH (n) RETURN ANY(x IN n WHERE x > 0)` |
| E5012 | builtin-arg-kind-mismatch | Emitted when a builtin's argument type is incompatible with its declared `ArgShape`. | `RETURN id(42)` |

## W6000–W6999 — Style / lint warnings

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| W6001 | dead-with | Projection with no downstream reader. | — |
| W6002 | reserved-keyword-identifier | Identifier collides with a reserved keyword; needs backtick quoting. | — |
| W6003 | duplicate-map-key | Duplicate key in map literal — last write wins. | — |
| W6004 | unused-variable | Variable bound but never read. | — |
| W6005 | redundant-optional-match | Redundant OPTIONAL MATCH — no bound variables escape the clause. | — |
| W6006 | unrestricted-pattern | Pattern has no label or type restriction — will scan broadly. | — |
| W6007 | inconsistent-keyword-casing | Inconsistent keyword casing inside one query. | — |
| W6010 | unreachable-label | Emitted by `cypher schema check` when a label is never referenced by any rel type's endpoint lists. | — |

## W7000–W7999 — Performance warnings

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| W7001 | cartesian-product | Cartesian product between disconnected MATCH components. | — |
| W7002 | expensive-call-in-filter | Expensive function call inside a row-wise filter. | — |
| W7003 | unbounded-var-length-path | Variable-length path without an upper bound. | — |
| W7004 | unindexed-selective-filter | Property access on an unindexed label in a selective filter. | — |

## N8000–N8999 — Informational notes

| Code | Name | Short description | Example |
| --- | --- | --- | --- |
| N8001 | pattern-normalised | Informational — pattern normalised to canonical direction. | — |
| N8002 | inferred-type | Informational — inferred type of an expression. | — |
| N8003 | variable-dropped | Informational — variable dropped from scope by this projection. | — |

---

## Per-code notes

Only codes whose one-line entry is genuinely insufficient are expanded
below.

### <a id="e0001"></a>E0001 — generic syntax error

The catch-all bucket for syntax errors that do not match any more
specific code. Its presence usually means the parser hit an
unrecoverable state before classifying the failure. Prefer a narrower
code where the grammar production is known; file a bead if you see
E0001 where a specific code would be clearer.

### <a id="e0011"></a>E0011 — expected `)` to close node pattern

Raised by the node-pattern production when a `(` is opened but the
matching `)` never appears before the next clause boundary. The typical
fix is to add the missing close paren.

```cypher
// before
MATCH (n:Person RETURN n

// after
MATCH (n:Person) RETURN n
```

### <a id="e0044"></a>E0044 — clause not yet implemented

A clause keyword was recognised but its production is deferred. This is
distinct from E0045 (unknown keyword) — the keyword is reserved and
registered, but the v1 parser intentionally does not consume it. See
spec §9.3 / §19 for the deferred-construct list.

### <a id="e0064"></a>E0064 — unclosed index bracket

Distinct from E0033 (legacy `SUBSCRIPT_EXPR` recovery). The typed
`index_or_slice_postfix` path on the cy-7s6.1 grammar uses E0064
exclusively so tooling can tell the two recovery paths apart. Fix by
adding the matching `]`.

```cypher
// before
RETURN xs[0

// after
RETURN xs[0]
```

### <a id="e3010"></a>E3010 — opaque / unresolved schema type

Emitted by `cypher schema check` (spec 0002 §9) when a property or
parameter declares a v0-opaque type such as `DURATION` or `POINT`.
Schema load still succeeds — the type round-trips — but the linter
surfaces it so operators know the value is treated as an opaque
symbolic handle only.

### <a id="e3011"></a>E3011 — self-referential relationship type

Emitted by `cypher schema check` when a relationship type declares the
same label in both `start_labels` and `end_labels`. The spec permits
self-loops (`KNOWS: Person → Person` is fine), but the linter raises
E3011 so operators can confirm the overlap is intentional rather than
a modelling slip (`REPORTS_TO: Team → Team`).

### <a id="e5003"></a>E5003 — type mismatch

Produced by `cypher-sema::unify::TypeMismatch::into_diagnostic` when
the unifier encounters two concrete types that cannot be reconciled
(spec §7.2 / §7.3). Call sites supply the source range; the code is
always `E5003`, so any two-type-mismatch surfaces uniformly.

---

## How this file stays in sync

- `cargo xtask gate` runs `check-diag-codes`, which enforces: every
  emitted code has a registry entry, every registered code is emitted
  (no dead code), and the total is pinned by
  `crates/cypher-diag/tests/registry.rs` (currently `EXPECTED = 120`).
- This doc is hand-maintained but count-checked. A future bead could
  auto-generate it from the registry — for now, when you add a code to
  `DiagCode`, append a row to the appropriate table above and (if
  warranted) a per-code note.
