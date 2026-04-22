# Error recovery table

Normative, per-production recovery strategy for `cypher-syntax`. Tracks
spec 0001 §4.3 (error recovery) and §17.18 (structural coverage check).

Every production named in `crates/cypher-ast/cypher.ungrammar` has an
entry below, keyed by an H3 heading whose text is the exact production
identifier. The set symmetry between this file and the ungrammar is
enforced by `cargo xtask check-recovery`, a blocking PR gate (§17.18).
To add a production: add its entry here. To remove one: delete the
ungrammar definition *and* this entry in the same commit.

Each entry carries three fields:

- **Synchronisation set** — tokens that, when reached, stop the skip.
  Always implicitly unioned with `RECOVERY_STOP` (clause-start keywords
  + `;` + EOF) from `crates/cypher-syntax/src/parser.rs`.
- **Skip-and-recover** — tokens consumed into an `ERROR` node while
  seeking the synchronisation set.
- **Virtual insertion** — closers the parser virtually inserts at the
  expected offset when missing (no bytes consumed, one diagnostic).

Entries tagged **DEFERRED** cover productions whose parser has not
landed yet (cy-nom implements MATCH / WHERE / RETURN and their direct
sub-productions only). They still participate in the coverage
invariant; the strategy will be fleshed out when the production lands.

## Statement-level productions

### SourceFile

- Synchronisation set: `;`, EOF.
- Skip-and-recover: any token that is not a clause-start keyword,
  wrapped in an `ERROR` node and continued statement-by-statement.
- Virtual insertion: none. Missing trailing `;` is tolerated by the
  ungrammar (`';'?`).

### Statement

- Synchronisation set: `UNION`, `;`, EOF, clause-start keywords.
- Skip-and-recover: default per spec §4.3 — up to the next clause-start
  or statement terminator.
- Virtual insertion: none.

### SingleQuery

- Synchronisation set: clause-start keywords, `UNION`, `;`, EOF.
- Skip-and-recover: tokens that are neither clause-start nor statement
  terminator.
- Virtual insertion: none.

### Union

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### UnionTail

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### Clause

- Synchronisation set: clause-start keywords, `;`, EOF.
- Skip-and-recover: on an unrecognised token at clause position, skip
  to the next clause-start / terminator.
- Virtual insertion: none.

## Reading clauses

### MatchClause

- Synchronisation set: `WHERE`, `,` (between patterns), clause-start
  keywords, `;`, EOF.
- Skip-and-recover: tokens inside an ill-formed pattern are swallowed
  into the `PATTERN_PART` or `NODE_PATTERN` `ERROR` node until a
  sync-set token is reached.
- Virtual insertion: `MATCH` keyword is required after `OPTIONAL` —
  a missing keyword emits a diagnostic but no token is inserted; the
  parser proceeds to `pattern_list`.

### WithClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### ReturnClause

- Synchronisation set: `ORDER`, `SKIP`, `LIMIT`, clause-start keywords,
  `;`, EOF.
- Skip-and-recover: tokens inside a malformed `ReturnItem` are consumed
  into the item's `ERROR` node until `,`, `ORDER`, `SKIP`, `LIMIT`, or
  a sync-set token is reached.
- Virtual insertion: none.

### UnwindClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### CallClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### YieldClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

## Writing clauses

### CreateClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### MergeClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### MergeAction

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### SetClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### RemoveClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### DeleteClause

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

## Shared sub-clauses

### WhereClause

- Synchronisation set: clause-start keywords, `ORDER`, `SKIP`, `LIMIT`,
  `;`, EOF, and any right-delimiter the parent construct is about to
  close (`]` for comprehensions, `)` for predicate expressions).
- Skip-and-recover: expression-body tokens when `expr` returns `None`
  are consumed until a sync-set token is reached.
- Virtual insertion: none.

### OrderBy

- Synchronisation set: `SKIP`, `LIMIT`, clause-start keywords, `;`, EOF.
- Skip-and-recover: malformed sort-items are wrapped in an `ORDER_ITEM`
  with an internal `ERROR` node; recovery continues at the next `,`
  or sync-set token.
- Virtual insertion: missing `BY` after `ORDER` emits a diagnostic;
  parser continues as if `BY` were present.

### SortItem

- Synchronisation set: `,`, `ASC`, `DESC`, `ASCENDING`, `DESCENDING`,
  `SKIP`, `LIMIT`, clause-start keywords, `;`, EOF.
- Skip-and-recover: expression tokens until a sync-set token.
- Virtual insertion: none.

### Skip

- Synchronisation set: `LIMIT`, clause-start keywords, `;`, EOF.
- Skip-and-recover: expression tokens until a sync-set token when the
  expression is missing.
- Virtual insertion: none.

### Limit

- Synchronisation set: clause-start keywords, `;`, EOF.
- Skip-and-recover: expression tokens until a sync-set token when the
  expression is missing.
- Virtual insertion: none.

### ReturnItems

- Synchronisation set: `ORDER`, `SKIP`, `LIMIT`, clause-start keywords,
  `;`, EOF.
- Skip-and-recover: malformed items skip to the next `,` or sync-set
  token; the parent `RETURN_ITEMS` node still closes.
- Virtual insertion: none.

### ReturnItem

- Synchronisation set: `,`, `ORDER`, `SKIP`, `LIMIT`, clause-start
  keywords, `;`, EOF.
- Skip-and-recover: tokens in a malformed expression or malformed `AS`
  alias are consumed into the item's `ERROR` node until a sync-set
  token.
- Virtual insertion: none. A missing identifier after `AS` emits a
  diagnostic only.

### YieldItem

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### SetItem

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PropertyAssign

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### LabelAdd

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### NodeReplace

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### NodeMerge

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### RemoveItem

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PropertyRemove

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### LabelRemove

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

## Patterns

### Pattern

- Synchronisation set: `,`, `WHERE`, clause-start keywords, `;`, EOF.
- Skip-and-recover: non-pattern tokens at pattern position are caught
  inside `PATTERN_PART`'s error branch.
- Virtual insertion: none.

### PathPattern

- Synchronisation set: `,`, `WHERE`, clause-start keywords, `;`, EOF.
- Skip-and-recover: a missing leading `(` aborts the path and emits
  a diagnostic; the partial node is closed as `PATTERN_PART` with an
  internal `ERROR`.
- Virtual insertion: none.

### ShortestPathPattern

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PathElement

- Synchronisation set: `,`, `WHERE`, clause-start keywords, `;`, EOF.
- Skip-and-recover: missing node pattern after a relationship breaks
  out of the chain with a diagnostic; already-parsed segments remain.
- Virtual insertion: none.

### NodePattern

- Synchronisation set: `)`, `,`, `WHERE`, clause-start keywords, `;`,
  EOF.
- Skip-and-recover: label/property tokens that do not parse fall into
  their sub-production's error recovery.
- Virtual insertion: missing `)` emits "expected ')' to close node
  pattern" at the expected offset; no byte is consumed, and the node
  closes as `NODE_PATTERN`.

### RelPattern

- Synchronisation set: `(` (next node pattern), `,`, `WHERE`,
  clause-start keywords, `;`, EOF.
- Skip-and-recover: missing opening `-` or `<-` emits a diagnostic;
  missing closing `-` / `->` likewise emits a diagnostic and does not
  consume further tokens.
- Virtual insertion: closing arrow / minus is virtually present for
  diagnostic purposes; no token is bumped.

### RelDetail

- Synchronisation set: `]`, `(`, `,`, `WHERE`, clause-start keywords,
  `;`, EOF.
- Skip-and-recover: bad tokens inside the brackets fall through to the
  sub-production; whole-bracket malformation exits via the missing
  `]` diagnostic.
- Virtual insertion: missing `]` emits "expected ']' to close
  relationship detail" at the expected offset.

### RangeHops

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### LabelExpr

- Synchronisation set: `{`, `)`, `]`, `,`, `WHERE`, clause-start
  keywords, `;`, EOF.
- Skip-and-recover: a missing label identifier after `:` breaks the
  loop with a diagnostic; the partial `LABEL_EXPR` still closes.
- Virtual insertion: none.

### TypeExpr

- Synchronisation set: `]`, `{`, `,`, clause-start keywords, `;`, EOF.
- Skip-and-recover: missing rel-type identifier after `:` emits a
  diagnostic; the partial `REL_TYPE_EXPR` still closes.
- Virtual insertion: none. Pipe-disjunction `A|B` is deferred.

### PropertyMap

- Synchronisation set: `}`, `)`, `]`, `,`, clause-start keywords, `;`,
  EOF.
- Skip-and-recover: malformed entries are handled by `PropertyKV`;
  whole-map malformation exits via the missing `}` diagnostic.
- Virtual insertion: missing `}` emits "expected '}' to close property
  map" at the expected offset.

### PropertyKV

- Synchronisation set: `,`, `}`, clause-start keywords, `;`, EOF.
- Skip-and-recover: missing key, `:`, or value each emit their own
  diagnostic; expression tokens inside a malformed value are consumed
  into the value's `ERROR` node until a sync-set token.
- Virtual insertion: none.

## Expressions

### Expr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### Literal

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### IntLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### FloatLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### StringLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### BoolLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### NullLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### ParameterExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### VariableRef

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PropertyAccess

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### IndexAccess

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### IndexExpr

- Status: **implemented** (cy-7s6.1).
- Synchronisation set: `]` closes the bracket; clause-level keywords +
  `;` + EOF act as outer recovery anchors.
- Skip-and-recover: an unclosed bracket emits `UNCLOSED_INDEX_BRACKET`
  (E0064) and closes the `INDEX_EXPR` node at the current token — the
  Pratt loop resumes on the next postfix / infix / clause boundary.
- Virtual insertion: none.

### SliceExpr

- Status: **implemented** (cy-7s6.1).
- Synchronisation set: `]` closes the bracket; `..` separates start
  from end inside it. Outside the bracket, default clause anchors
  apply.
- Skip-and-recover: same as `IndexExpr` — missing `]` emits
  `UNCLOSED_INDEX_BRACKET` (E0064). Either bound may be elided (produces
  a `SLICE_EXPR` with one or zero child expressions), which is the spec
  shape and not a diagnostic.
- Virtual insertion: none.

### FunctionCall

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### ArgList

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### CountStar

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### ParenExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: missing `)` will emit "expected ')'" at the
  expected offset once the production lands.

### ListLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: missing `]` will emit "expected ']'" at the
  expected offset once the production lands.

### MapLiteral

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: missing `}` will emit "expected '}'" at the
  expected offset once the production lands.

### ListComprehension

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PatternComprehension

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### CaseExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### WhenArm

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### NullCheck

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### ExistsPredicate

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PatternPredicate

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### BinaryExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### BinaryOp

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### UnaryExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### UnaryOp

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### StringPatternExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### StringPatternOp

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### MembershipExpr

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

## Names

### NameDef

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### NameRef

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### QualifiedName

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### Label

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### RelType

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.

### PropertyKey

- Status: **DEFERRED** — grammar not yet implemented (see cy-nom scope / follow-up bead).
- Synchronisation set: clause-level keywords + `;` + EOF (default).
- Skip-and-recover: default per spec §4.3.
- Virtual insertion: none planned until production lands.
