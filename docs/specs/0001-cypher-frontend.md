# Spec 0001 — Standalone Rust Cypher Front-End

| Field          | Value                                                                 |
| -------------- | --------------------------------------------------------------------- |
| Status         | Accepted                                                              |
| Owner          | phall                                                                 |
| Last locked    | 2026-04-18                                                            |
| Supersedes     | —                                                                     |
| Superseded by  | —                                                                     |

---

## Amendment log

Post-lock edits to this spec. Each entry cites the bead that carried the
change and the operator approval that unlocked it. The spec body is
otherwise verbatim as of the last-locked date above.

- **2026-04-22 — cy-wmc** (operator-approved in session, 2026-04-22). §3.1
  crate table: add `cypher-project` to the allowed dependencies of
  `cypher-lang-services` and `cypher-lsp`. Rationale: cy-o8c tranche 2
  (cross-file LSP navigation, bead cy-kkw) needs the workspace project
  model inside the lang-services engine; making `cypher-lsp`'s edge to
  `cypher-project` explicit keeps §3.1 the single source of truth even
  though the dep is already reachable transitively via `cypher-lang-services`.
  This amendment also reconciles §3.1 with AGENTS.md §3 by landing the
  previously-implicit rows for `cypher-project` (spec 0003) and
  `cypher-lang-services` (the shared LSP/agent engine described in §3.2);
  those crates already exist under `crates/`, were documented in AGENTS.md §3,
  and are now mirrored here so the spec is authoritative.

---

## 0. TL;DR

A Rust-native Cypher **front-end platform** — lexer, recovering parser, lossless
CST, typed AST, HIR with name resolution, schema-aware semantic analysis,
diagnostics engine, incremental analysis database, formatter, LSP server,
agent-facing JSON API, and CLI. **No execution.** **No domain coupling.** The
workspace lives at `trench.lang/cypher/` but could be extracted to its own git
repo at any moment; trench, Neo4j, Memgraph, and Kuzu are equivalently-weighted
downstream consumers from the library's point of view.

The bar is rust-compiler-grade: compiletest-style golden tests, snapshot tests,
property tests, fuzzing, mutation testing, openCypher TCK conformance,
differential testing against `libcypher-parser`, criterion benchmarks with
regression gates, `#![forbid(unsafe_code)]` workspace-wide, and a CI matrix
that spans stable/beta/nightly × Linux/macOS/Windows.

---

## 1. Problem & Goals

### 1.1. Problem

Existing Rust Cypher parsing is hobby-grade. The serious Cypher tooling lives
outside Rust (Neo4j's Scala/Java front-end, `libcypher-parser` in C, ANTLR
grammars). A Rust-native Cypher front-end that competes with those stacks does
not yet exist, and a best-in-class implementation is the unlock for:

- agent-authored query workflows where the LLM edits Cypher in a sandbox and
  receives span-accurate diagnostics, completions, and quick-fixes on every
  keystroke;
- tight, Rust-native consumers (graph stores, analytic engines) that want to
  build on a typed Plan IR rather than a string-shaped query;
- IDE-grade editor integration that survives broken code gracefully.

### 1.2. Goals

G1. Parse every query in the openCypher TCK for the supported clause surface
(§19), and every variant those tests exercise, without panicking on any input.
G2. Produce a lossless CST that preserves every byte of the input (comments,
trivia, malformed fragments) and is usable for formatters and range-accurate
editor tooling.
G3. Produce a typed AST and a lowered HIR with resolved names, resolved types
(where schema is available), desugared syntactic sugar, and stable node IDs.
G4. Emit rustc-grade diagnostics: stable error codes, severities, labeled
spans, notes, fix-its, related information.
G5. Run incrementally: edit a 10,000-line query file and only reparse /
recheck what changed.
G6. Ship an LSP server that a Neo4j user, a Memgraph user, or a trench user
can plug into VS Code today.
G7. Ship an agent-JSON API for LLM-driven workflows that gives single-call
access to parse / diagnose / complete / rewrite / plan.

### 1.3. Non-goals (v1)

N1. No execution. Consumers execute the Plan IR.
N2. No domain knowledge.
N3. No Neo4j-dialect compatibility in v1 (can be added by a future spec).
N4. No subquery support (CALL { ... }, EXISTS { ... }) in v1.
N5. No LOAD CSV, SHOW, ALTER, user-defined procedure/function registry in v1.
N6. No cost-based optimizer; the Plan IR is logical, not physical.
N7. No spatial types, no temporal types beyond `DATE` and `DATETIME`.

### 1.4. Success criteria

The spec is implemented when all of:

- openCypher TCK parse + semantic suites pass for the scope matrix in §19.
- `cargo fuzz` runs for 24h with no panics, no hangs, no OOMs on any corpus
  target (lexer, parser, formatter).
- Property suite (parse-print-parse round-trip, formatter idempotence,
  parser-total on arbitrary bytes) passes 10⁶ cases.
- `cargo mutants` kill rate ≥ 90% on the semantic analyzer.
- Criterion parse throughput ≥ 10 MiB/s on a reference M-series Mac on a
  realistic corpus.
- Golden compiletest corpus is stable across runs (byte-identical stderr).
- LSP passes the `lsp-types` conformance harness and a custom integration
  suite covering completion, hover, diagnostics, rename, formatting.
- Agent JSON API round-trips the same operations end-to-end.

---

## 2. Non-Coupling Contract

Load-bearing invariants. Any future change to this workspace must preserve all
of them, or it is not permitted.

C2.2. No module, type, or function in this workspace names a domain concept
from trench (no `Actor`, `Event`, `Operation`, `Capability`, `provenance`,
`branch`, `bitemporal`, `expertise`, etc.). Grep-enforced in CI with a
denylist.

C2.3. Domain-specific extensions (schema, custom functions, write-clause
semantics, temporal extensions, auditing) are expressed by consumers
implementing the trait interfaces in §8. The consumer crate lives outside this
workspace. No "overlay crate" is permitted inside this workspace.

C2.4. The workspace is published-shaped from day one: `README.md`,
`LICENSE-APACHE`, `LICENSE-MIT`, crate-level docs, `docs.rs`-clean. If trench
disappeared tomorrow, the workspace would still be releasable on crates.io.

C2.5. MSRV and toolchain pinning live in the cypher workspace's own
`rust-toolchain.toml`. 

---

## 3. Crate Graph

### 3.1. Crates

| Crate            | Purpose                                                                              | Depends on                                          |
| ---------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------- |
| `cypher-syntax`  | Lexer, event-based recovering parser, lossless CST (rowan green/red tree), SyntaxKind | `rowan`, `logos`, `smol_str`                        |
| `cypher-ast`     | Typed AST wrappers over CST nodes, generated from an `ungrammar`-style description    | `cypher-syntax`                                     |
| `cypher-hir`     | Lowered HIR, name resolution, scope graph, desugaring                                 | `cypher-ast`, `cypher-syntax`                       |
| `cypher-sema`    | Semantic passes (schema-free + schema-aware), type system, aggregation scope          | `cypher-hir`, `cypher-schema`                       |
| `cypher-schema`  | `SchemaProvider` trait, schema types, function/procedure catalog types                | `cypher-syntax` (for types only)                    |
| `cypher-diag`    | Diagnostic type, code registry, severity, labels, fix-its, rendering backends         | `cypher-syntax`                                     |
| `cypher-plan`    | Read-plan and write-plan logical IR, lowering from HIR                                | `cypher-hir`                                        |
| `cypher-fmt`     | CST-driven formatter                                                                 | `cypher-syntax`                                     |
| `cypher-db`      | Salsa-based incremental analysis database tying the above                            | `cypher-syntax`, `cypher-hir`, `cypher-sema`, `cypher-plan`, `cypher-schema`, `cypher-diag`, `salsa` |
| `cypher-project` | Workspace project manifest loader (`cypher-project.toml`): members, dialect defaults, lint levels, schema wiring (spec 0003) | `cypher-schema`, `smol_str`, `thiserror`, `serde`, `toml`, `globset`, `walkdir` |
| `cypher-lang-services` | Shared completion / hover / rewrite engines keyed on `(db, file_id, byte-offset)`; thin adapter target for LSP and agent | `cypher-db`, `cypher-hir`, `cypher-schema`, `cypher-sema`, `cypher-syntax`, `cypher-ast`, `cypher-fmt`, `cypher-project` |
| `cypher-lsp`     | Language server binary (stdio + TCP)                                                 | `cypher-lang-services`, `cypher-db`, `cypher-diag`, `cypher-fmt`, `cypher-project`, `lsp-server`, `lsp-types` |
| `cypher-agent`   | JSON-over-stdio agent API binary                                                     | `cypher-lang-services`, `cypher-db`, `cypher-diag`, `cypher-fmt`, `serde_json` |
| `cypher-cli`     | CLI binary: `cypher {parse,check,fmt,explain,plan}`                                   | `cypher-db`, `cypher-diag`, `cypher-fmt`, `cypher-schema`, `cypher-project` |
| `cypher-tck`     | openCypher TCK harness                                                               | `cypher-db`                                         |
| `cypher`         | Meta-crate re-exporting the library surface for convenience                          | all non-binary crates above                         |

### 3.2. Dependency rules

- Any crate may depend on its left-of-column predecessors only (roughly: no
  cycles, semantic layering).
- No crate above `cypher-db` may depend on `salsa`. Incrementality is an
  integration concern; each pure layer must be usable without it.
- Binary crates (`cypher-lsp`, `cypher-agent`, `cypher-cli`) are thin shells
  over `cypher-db`. No analysis logic lives in binaries.
- Test-only code: `cypher-testkit` (not published) provides shared fixtures,
  snapshot helpers, and the compiletest runner.

### 3.3. Rationale

The split matches the consumer use cases directly. A library consumer that
only wants to parse and format does not pull in `salsa`. A consumer building
an IDE pulls `cypher-db` + `cypher-lsp`. A consumer building a query planner
for their own graph store pulls `cypher-db` + their own executor. The meta-
crate `cypher` exists for the 90% case (parse-to-plan) where consumers want
one dependency and care less about surface minimization.

---

## 4. Syntax Layer (`cypher-syntax`)

### 4.1. Lexer

Built with [`logos`](https://github.com/maciejhirsz/logos). Declarative token
definitions produce a DFA; the lexer is zero-copy over the input and runs at
hundreds of MiB/s on current hardware. Every token carries its byte range.

Token classes:

- Keywords — case-insensitive match, original case preserved as text.
  Reserved vs contextual distinction follows openCypher (see §9 for dialect
  differences).
- Identifiers — ASCII-alphanumeric plus underscore, starting non-digit.
- Quoted identifiers — backtick-delimited; escape by doubling backtick.
- String literals — single- or double-quoted; escape sequences `\\`, `\'`,
  `\"`, `\t`, `\n`, `\r`, `\b`, `\f`, `\u{XXXX}`, `\U{XXXXXXXX}`.
- Numeric literals — integer (decimal, `0x` hex, `0o` octal, `0b` binary),
  float (with optional exponent). `Inf`, `NaN` as contextual idents, not
  literals in v1.
- Parameters — `$ident` or `$<decimal>`.
- Punctuation — `(`, `)`, `[`, `]`, `{`, `}`, `,`, `;`, `:`, `::`, `.`, `..`,
  `|`, `*`, `+`, `-`, `/`, `%`, `^`, `=`, `<>`, `!=`, `<`, `<=`, `>`, `>=`,
  `->`, `<-`, `=~`, `$`.
- Comments — `//` line, `/* ... */` block (nested not supported; mirrors
  openCypher).
- Whitespace — space, tab, CR, LF.
- Error token — any byte the DFA cannot consume; carries the offending range.

All non-significant tokens (whitespace, comments) are retained as **trivia**
attached to the following significant token (leading) or to the preceding
token if at end of file (trailing). Trivia is preserved in the CST; it is
invisible to AST and HIR consumers but round-trips through the formatter.

### 4.2. Parser

Hand-written event-based recursive-descent with Pratt precedence for
expressions. The parser emits an event stream (`Start(kind)`, `Token`,
`Finish`, `Error(diag_id)`); a builder consumes the events and constructs the
rowan green tree. Separation of event emission from tree construction lets us
rewrite the event stream for associativity fixing and error grouping before
we build the tree.

Choice of hand-written over `pest` / `lalrpop` / ANTLR:

- Error recovery quality: hand-written parsers produce vastly better partial
  trees under errors. Every mature production front-end (rust-analyzer, ruff,
  swc, TypeScript, Roslyn, Clang) is hand-written.
- Performance: no generated-table overhead, no runtime grammar interpreter.
- Incremental friendliness: event-based parsers can restart sub-parses on
  small edits.
- Maintenance: a Cypher grammar file is not an asset we gain by using a
  parser generator; we lose tree control.

### 4.3. Error recovery

Recovery strategy is explicit and documented per grammar production. Two
primitives:

- **Synchronization set per clause.** When parsing fails inside a clause,
  skip tokens until we hit a clause-level keyword (`MATCH`, `OPTIONAL`,
  `WHERE`, `WITH`, `RETURN`, `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`,
  `UNWIND`, `CALL`) or a statement terminator (`;`, EOF). Insert an ERROR
  node in the tree wrapping the skipped tokens.
- **Expected-token insertion.** For missing closers (`)`, `]`, `}`), the
  parser virtually inserts the expected token, records the diagnostic at the
  expected position, and continues. The inserted token is an ERROR node with
  zero width.

For every grammar production, the spec-accompanying recovery table lists:
which tokens are synchronization points, which tokens are "skip-and-recover",
which tokens trigger virtual insertion. The table is a normative part of this
spec and lives in `cypher-syntax/docs/recovery.md` (to be written as part of
implementation).

**Coverage invariant.** Every production named in `cypher-ast/cypher.ungrammar`
MUST have an entry in `recovery.md`. Entries for productions that no longer
exist in the grammar MUST be removed. This invariant is enforced structurally
by `cargo xtask check-recovery`, which parses both files and fails with a
diff on any asymmetry (§17.18). The check is a blocking PR gate; drift is not
a "weekly cargo-mutants will catch it" problem.

### 4.4. CST

Rowan green/red tree. `SyntaxKind` is a hand-enumerated non-exhaustive enum
covering all node kinds and all token kinds (rowan pattern). A dedicated
source file `src/kind.rs` holds the `SyntaxKind` definition; it is the
canonical grammar reference.

The CST is lossless: `cst.syntax().to_string() == original_input` is an
invariant asserted in property tests.

### 4.5. Span model

Every CST node has a `TextRange` (byte offsets into the input). Offsets are
UTF-8 byte offsets; UTF-16 conversion happens at the LSP boundary only. A
`LineIndex` type (mirroring rust-analyzer) converts byte offsets to
line/column for diagnostic rendering.

### 4.6. Statement boundaries

A source file can contain multiple statements separated by `;`. The parser
produces one CST per file, with `SyntaxKind::Statement` children. Trailing
`;` is optional. An empty file parses to an empty tree, not an error.

---

## 5. AST Layer (`cypher-ast`)

### 5.1. Wrappers, not owned values

Typed AST nodes are zero-cost wrappers around `SyntaxNode`. Each AST node is
a single-field struct holding a rowan `SyntaxNode`; its methods navigate the
underlying tree. No allocations at AST construction; no data duplication. The
rust-analyzer pattern.

### 5.2. Generation from grammar description

The AST boilerplate (hundreds of wrapper types, field accessors, `AstNode`
impls) is generated from a `cypher.ungrammar`-shaped description by a
dev-only `xtask`. The generated file is committed (not built at compile
time); CI verifies the generated file matches what regeneration would
produce.

### 5.3. Missing fields are `Option`

A node's accessor returns `Option<ChildNode>` when the grammar allows
absence, even if "valid" Cypher requires presence. This lets the AST work
over partial/broken code without asserting.

### 5.4. Enum wrappers for sum types

Alternation in the grammar (e.g., `Expression = LiteralExpr | BinaryExpr |
CallExpr | ...`) becomes an enum of the wrapper types. `AstNode::cast` is
the conversion function.

---

## 6. Name Resolution & HIR (`cypher-hir`)

### 6.1. HIR shape

HIR is an owned, resolved representation. Lowering from AST into HIR happens
in one pass per statement. HIR node identity is a `HirId` (statement-scoped
index). The AST ↔ HIR map is preserved for span-accurate diagnostics.

HIR nodes:

- `Statement` — root per statement.
- `Clause` — MATCH, OPTIONAL_MATCH, WHERE, WITH, RETURN, CREATE, MERGE, SET,
  REMOVE, DELETE, UNWIND, CALL (v1 scope).
- `Pattern` — a parsed graph pattern, broken into its connected components.
- `PatternElement` — node or relationship.
- `Expr` — literal, variable ref, property access, call, binary, unary,
  list/map, comprehension, case, pattern predicate.
- `Projection` — a single output of RETURN or WITH.
- `VarId` — interned variable identity within a statement.

Syntactic sugar is desugared during lowering:

- List comprehensions (`[x IN xs WHERE p(x) | e(x)]`) → explicit filter/map
  over the iterable.
- Pattern predicates in expressions (`MATCH (a) WHERE (a)-->(:X)`) → explicit
  existential subquery at HIR level, flagged as `ExprKind::PatternPredicate`
  so downstream passes recognize it.
- Map projection (`a { .name, .age, computed: f(a) }`) → explicit map
  construction.
- Shorthand property matching (`MATCH (a {name: $n})`) → lowered to
  `MATCH (a) WHERE a.name = $n`.

### 6.2. Name resolution

Cypher scoping in v1:

- Each statement introduces a fresh scope stack.
- Pattern variables in a `MATCH` / `OPTIONAL MATCH` / `CREATE` / `MERGE` are
  visible from the moment they bind through the end of the statement or until
  a `WITH` clause intervenes.
- `WITH` is a scope barrier: only the variables explicitly projected (or
  renamed) through `WITH` are visible to the following clauses. Everything
  else falls out of scope.
- `RETURN` is terminal; variables it projects are the query's output
  signature, not a new scope.
- `UNWIND <expr> AS v` binds `v` in the enclosing scope after the clause.
- `CALL ... YIELD a, b` binds `a`, `b` (v1 supports standalone CALL; YIELD-
  binding is part of the CALL grammar).

Name resolution produces:

- A `ScopeGraph` — node: scope; edge: parent/child; payload: bindings in
  scope.
- A `ResolvedNames` map from each use-site `HirId` to its definition
  `HirId`, or to an `Unresolved` marker.
- Shadowing is reported as a diagnostic (warning by default; configurable).

### 6.3. Variable kinds

A bound variable has a kind recorded at binding:

- `Node` (bound by a node pattern),
- `Relationship` (bound by a relationship pattern),
- `Path` (bound by `p = MATCH (...)-[]->(...)`),
- `Value` (bound by `UNWIND ... AS v`, `WITH expr AS v`, `CALL ... YIELD v`).

Kind mismatches (e.g., `a.name` where `a` is bound as a path) are diagnostic
errors at semantic analysis, not at name resolution.

---

## 7. Semantic Analysis (`cypher-sema`)

### 7.1. Two modes, one pipeline

- **Schema-free.** Always run. Catches what does not need schema:
  unresolved variables, kind mismatches, aggregation scope violations,
  illegal clause ordering, duplicate relationship variables, DISTINCT with
  aggregation rules, ORDER BY over invisible variables, bad parameter
  references, type errors that are structural (e.g., indexing a boolean,
  arithmetic on lists).
- **Schema-aware.** Run additionally when a `SchemaProvider` is supplied.
  Adds: unknown labels, unknown relationship types, unknown properties,
  property type mismatches, relationship endpoint mismatches, unknown
  functions, function-arity / function-type mismatches, unknown procedures.

Both modes share the diagnostic pipeline; the user cannot tell from a
diagnostic's surface which mode produced it except by its error code.

### 7.2. Type system

Types, expressed in the `cypher_sema::Type` enum:

```
Type = Any
     | Null
     | Bool
     | Int
     | Float
     | Num             // Int ∨ Float, used in unification
     | String
     | Date
     | Datetime        // v1 temporal cap (§19)
     | List(Box<Type>)
     | Map(BTreeMap<String, Type>)   // structural when known, Any-valued otherwise
     | Node(Option<LabelSet>)
     | Relationship(Option<TypeName>)
     | Path
     | Union(Vec<Type>)               // for RETURN over alternation branches
     | Unknown                        // inference failure marker
```

Operations are typed via a small unification engine. `Any` is the universal
subtype (queries without schema produce Any-typed property reads; only
structural errors surface). `Num` unifies to whichever of Int/Float is
reachable; division of Int by Int is Int in openCypher (no automatic
promotion) — the spec codifies this exactly.

### 7.3. Aggregation scope

Aggregations are legal only in `RETURN`, `WITH`, and `ORDER BY` of the same
projection. When an aggregation appears in a projection list:

- Every non-aggregated expression in the same projection list is an
  **implicit grouping key**.
- Aggregations cannot be nested (`count(count(*))` is an error).
- Aggregations cannot appear in `WHERE`, `UNWIND`, pattern predicates, or
  function arguments that are themselves aggregations.
- `DISTINCT` in a projection applies to the non-aggregated keys; `count(DISTINCT x)`
  is an inline form, tracked separately.

Every rule in this subsection has its own diagnostic code.

### 7.4. Clause ordering

The clause sequence in a statement is constrained. v1 canonical ordering:

```
Statement = ReadingClause* UpdatingClause* (WITH ReadingClause* UpdatingClause*)* Terminal
ReadingClause = MATCH | OPTIONAL_MATCH | UNWIND | CALL
UpdatingClause = CREATE | MERGE | SET | REMOVE | DELETE
Terminal = RETURN | (UpdatingClause+ without RETURN)
```

Violations (e.g., `CREATE` followed by `MATCH` without an intervening
`WITH`; `RETURN` followed by another clause; `OPTIONAL MATCH` after a pure
write prefix) are diagnostics with dedicated codes.

### 7.5. Pattern-level validations

- A pattern variable can appear in multiple positions, but its kind must be
  consistent (a variable cannot be both a node and a relationship).
- A relationship variable cannot appear in more than one relationship slot
  in the same MATCH.
- Relationship variables in `CREATE` must be single-hop (no variable-length
  relationships).
- `MERGE` must have deterministic pattern (schema-aware: endpoints typed
  sufficiently; schema-free: structural rules).

### 7.6. Parameter discipline

Parameters are typed by their use-sites. If a parameter is used in multiple
contexts requiring incompatible types (e.g., once as a string, once as a
node), it is a diagnostic error. A consumer may supply parameter types
externally via `SemaOptions::parameter_hints` to typecheck in advance.

---

## 8. Schema Provider (`cypher-schema`)

### 8.1. Trait

```rust
pub trait SchemaProvider: Send + Sync + 'static {
    // ---- labels & relationship types ---------------------------------
    fn labels(&self) -> Vec<SmolStr>;
    fn relationship_types(&self) -> Vec<SmolStr>;
    fn has_label(&self, name: &str) -> bool;
    fn has_relationship_type(&self, name: &str) -> bool;

    // ---- properties ---------------------------------------------------
    /// Properties declared on a node with this label. Returns None if the
    /// label is unknown; returns Some(empty) if the label is known but has
    /// no declared properties.
    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>>;
    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>>;

    // ---- relationship shape ------------------------------------------
    /// Declared endpoint pairs for a relationship type. A relationship type
    /// may have multiple allowed endpoint pairs; returning empty means
    /// "endpoint-polymorphic" and the semantic pass skips endpoint checks.
    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl>;

    /// Declared inverse relationship type, if any. Used by normalisation
    /// passes and by completion.
    fn inverse_of(&self, rel_type: &str) -> Option<SmolStr>;

    // ---- callables ---------------------------------------------------
    fn function(&self, name: &str) -> Option<FunctionSignature>;
    fn procedure(&self, name: &str) -> Option<ProcedureSignature>;

    // ---- identity ----------------------------------------------------
    /// A content-addressed digest of the schema's observable surface.
    /// MUST change whenever any visible declaration changes; MUST be stable
    /// across identical schemas. Used as a Salsa input for incremental
    /// invalidation.
    fn schema_digest(&self) -> [u8; 32];
}
```

### 8.2. Supporting types

- `PropertyDecl { name: SmolStr, ty: PropertyType, required: bool }`
- `PropertyType` — `String | Int | Float | Bool | Date | Datetime |
  List(Box<PropertyType>) | Enum(SmolStr, Vec<SmolStr>) | Opaque(SmolStr)`.
  The `Opaque` variant carries a symbolic name for domain-specific types
  the consumer chooses not to model structurally; opaque types unify only
  with themselves and with `Any`.
- `EndpointDecl { from: SmolStr, to: SmolStr, cardinality: Cardinality }`
- `FunctionSignature { params: Vec<ParamDecl>, return_ty: Type,
  variadic: Option<Type>, categories: FnCategories }`
- `ProcedureSignature { params: Vec<ParamDecl>, yields: Vec<YieldDecl>,
  mode: ProcMode /* Read | Write | Schema */ }`

### 8.3. Built-in catalog

A crate-shipped `StandardLibrary` impl supplies the openCypher-standardized
function set: `id()`, `type()`, `labels()`, `keys()`, `properties()`, the
`length()` family, `size()`, `coalesce()`, the string functions, the
collection functions, the math functions, the aggregation functions. This is
independent of any consumer schema; a consumer wraps their own
`SchemaProvider` with `StandardLibrary::wrap(my_schema)` to get both.

### 8.4. Schema-less mode

When no `SchemaProvider` is supplied, semantic analysis runs in schema-free
mode (§7.1). The built-in `StandardLibrary` is still consulted for
functions.

### 8.5. Schema freshness & incrementality

`schema_digest()` is the Salsa input for schema-dependent queries. When the
consumer mutates its schema, it MUST update the digest; the analysis DB
invalidates downstream queries on digest change. Consumers that keep schema
static can return a constant digest.

---

## 9. Dialect Modes

### 9.1. Modes

v1 supports two modes, selected at parse time via
`ParseOptions::dialect: DialectMode`:

```rust
pub enum DialectMode {
    /// Canonical: GQL-aligned Cypher per ISO/IEC 39075. Where GQL and
    /// Neo4j-style Cypher differ, we follow GQL.
    GqlAligned,

    /// Compatibility: openCypher v9 per the openCypher spec + TCK.
    OpenCypherV9,
}
```

### 9.2. Mode-influenced behaviours

| Concern                              | `GqlAligned`                        | `OpenCypherV9`                |
| ------------------------------------ | ----------------------------------- | ----------------------------- |
| Reserved keyword set                 | GQL reserved + Cypher-core reserved | openCypher v9 reserved        |
| Quoted-identifier escape             | double-backtick                     | double-backtick               |
| `:` in label expressions             | `A|B` allowed; `!` allowed          | `A|B` only                    |
| Relationship detail in CREATE        | strict single-hop                   | strict single-hop             |
| `MERGE ... ON CREATE/ON MATCH SET`   | allowed                             | allowed                       |
| `NULL` comparison with `=`           | always `NULL`                       | always `NULL`                 |
| Integer division promotion           | `DIV` keyword for floor-divide      | `/` with integer operands     |
| `RETURN *` over empty scope          | diagnostic                           | diagnostic                    |

Dialect differences are called out in every affected parser and semantic
rule with a `DialectGate` construct; the gate records which modes allow the
construct and emits a specific diagnostic in the rejecting mode.

### 9.3. Not in v1

Neo4j-current dialect (`cypher 5`/`cypher 25`) features that are not part of
GQL or openCypher v9 (procedure schema, APOC imports, `CYPHER` prefixes,
CALL-in-transactions, `EXISTS {}` subqueries, etc.) are deferred. A future
spec may add `DialectMode::Neo4jCurrent`.

---

## 10. Diagnostics (`cypher-diag`)

### 10.1. Diagnostic type

```rust
pub struct Diagnostic {
    pub code: DiagCode,                 // stable, e.g. "E0301"
    pub severity: Severity,             // Error | Warning | Note | Help
    pub message: SmolStr,               // one-line summary
    pub primary: Label,                 // main span
    pub labels: Vec<Label>,             // secondary spans with captions
    pub notes: Vec<SmolStr>,            // trailing explanatory lines
    pub related: Vec<Related>,          // (file, range, msg) cross-refs
    pub fixes: Vec<FixIt>,              // suggested edits
}
```

### 10.2. Code scheme

| Range       | Meaning                                           |
| ----------- | ------------------------------------------------- |
| `E0001–E0999` | Syntax (lexer + parser)                          |
| `E1000–E1999` | Name resolution                                  |
| `E2000–E2999` | Semantic — schema-free                           |
| `E3000–E3999` | Semantic — schema-aware                          |
| `E4000–E4999` | Dialect / compatibility                          |
| `E5000–E5999` | Type system                                      |
| `W6000–W6999` | Style / lint warnings                            |
| `W7000–W7999` | Performance warnings (predictable red flags)     |
| `N8000–N8999` | Informational notes emitted by the analyser      |

Codes are **stable**. Once assigned, a code's meaning cannot change. New
checks get new codes; removed checks leave their code retired, never reused.
A registry file `cypher-diag/src/codes.rs` is the source of truth; CI fails
on duplicate codes or on codes referenced in tests but not registered.

### 10.3. Rendering backends

- Plain-text (default, terminal with ANSI) — codespan-reporting under the
  hood.
- JSON — one-object-per-diagnostic, stable field names, used by `cypher-agent`.
- LSP — produced directly from `Diagnostic` via a small converter.
- SARIF — deferred (trivial to add; not v1).

### 10.4. Accumulation model

A single analysis run emits many diagnostics. Each pass accumulates into a
sink, the sink is drained at the end, and diagnostics are sorted by primary
span offset. No pass short-circuits on first error. Rendering caps the
"noise tail" (configurable) but diagnostics below the cap are still
retrievable programmatically.

### 10.5. Fix-its

A `FixIt` is a set of `TextEdit { range, replacement }` tuples. Fix-its
carry an `Applicability`: `MachineApplicable`, `MaybeIncorrect`,
`HasPlaceholders`, `Unspecified`. LSP and CLI consume fix-its directly;
agent API exposes them as JSON.

### 10.6. Related information

Related entries carry a `TextRange` and an optional file reference (for
multi-file futures, a no-op in v1). They support cross-referencing the
definition of a shadowed variable, the conflicting aggregation, the schema
declaration that mismatched, etc.

---

## 11. Incremental Query Database (`cypher-db`)

### 11.1. Choice: Salsa

Salsa 2022-style API (exact version pinned in `Cargo.toml`; upgrade is a
spec-governed change). Inputs are `#[input]` queries; derived analyses are
`#[tracked]` queries. The Salsa pattern is the de facto standard for
incremental Rust compilers (rust-analyzer, Biome, Astral's tooling).

### 11.2. Inputs

- `source_text(FileId) -> Arc<str>`
- `dialect(FileId) -> DialectMode`
- `schema_digest() -> [u8; 32]`
- `semantic_options() -> SemaOptions`

### 11.3. Derived queries

- `parse(FileId) -> Parse` — CST + parse diagnostics
- `ast(FileId) -> Ast`
- `hir(FileId) -> Hir` — lowered, named
- `sema(FileId) -> SemaResult` — schema-free + schema-aware
- `plan(FileId) -> PlanResult`
- `diagnostics(FileId) -> Arc<[Diagnostic]>`
- `formatted(FileId, FmtOptions) -> Arc<str>`

All queries are memoized and invalidated precisely on their input's change.
Salsa's LRU / durability controls are exposed via `Database::options`.

### 11.4. Multi-file

A `FileId` is the unit of caching. There is no cross-file import in v1
(Cypher has no import system), but `FileId` is the right granularity for
IDE workflows where the user edits many files concurrently. A
`SchemaProvider` is workspace-scoped, not per-file.

### 11.5. Concurrency

`Database` is `Send + Sync` via Salsa's built-in snapshot model. The LSP
server uses one `Database` and issues a snapshot per request; the CLI uses
one `Database` per process.

### 11.6. Memoization bounds

Long-running embedders (`cypher-lsp`, `cypher-agent`) field many queries
over a single process lifetime. Unbounded Salsa memo tables leak. Three
bounds are normative:

- **Per-query LRU caps.** The expensive derived queries (`parse`, `hir`,
  `sema`, `plan`, `formatted`) carry an explicit `lru = N` cap (Salsa's
  `#[tracked(lru)]`). Defaults: `N = 256`. Configured via
  `Database::options` at construction; not mutable after construction.
  Cheap queries (`ast`, `diagnostics`) are uncapped (their memo cost is
  dominated by their upstream, which is already capped).
- **LSP FileId eviction.** `cypher-lsp` MUST evict a `FileId` on
  `textDocument/didClose` when the document is not referenced by any
  open document. Eviction calls Salsa's input-removal API; memoized
  derived values keyed on that `FileId` are reclaimed on the next
  revision bump.
- **Agent FileId reuse.** `cypher-agent` is stateless-per-call: each
  request carries `{text}` rather than a `FileId` handle. The server MUST
  intern all requests onto a single `FileId` per dialect (the text
  changes; the `FileId` does not), so memoization budget is bounded by
  the LRU caps above rather than by request count. `schema_set` /
  `schema_clear` trigger `schema_digest` invalidation, not `FileId`
  churn.

Not normative: the CLI, which runs one `Database` per process and exits.
The bounds above prevent monotonic growth in daemonised embedders;
the CLI has no such pressure.

Benchmarked: `bench_incremental` (§17.10) includes a "10k edits" workload
that asserts steady-state RSS stays within ±10% of the first-1k-edits
baseline, under both LSP (FileId churn) and agent (single FileId)
access patterns.

---

## 12. Plan IR (`cypher-plan`)

### 12.1. Shape

Logical plan. No cost, no cardinality, no physical operator selection. The
plan is a directed acyclic graph of operators; each operator consumes rows
and produces rows, with typed column schemas.

```rust
pub enum ReadOp {
    /// All nodes with the given label (or all nodes if None).
    Source { label: Option<LabelSet>, bind: VarId },
    /// Join predicate expanded into a relationship traversal.
    Expand { input: OpId, from: VarId, rel: RelSpec, to: NodeSpec, bind_rel: VarId, bind_to: VarId },
    Filter { input: OpId, predicate: Expr },
    Project { input: OpId, items: Vec<Projection> },
    Aggregate { input: OpId, keys: Vec<Expr>, aggs: Vec<AggExpr> },
    OrderBy { input: OpId, keys: Vec<OrderKey> },
    Skip { input: OpId, count: Expr },
    Limit { input: OpId, count: Expr },
    Distinct { input: OpId },
    Unwind { input: OpId, list: Expr, bind: VarId },
    Union { left: OpId, right: OpId, kind: UnionKind },    // v1: all | distinct
    With { input: OpId, items: Vec<Projection>, filter: Option<Expr> },
    OptionalJoin { input: OpId, pattern: Box<ReadOp> },     // OPTIONAL MATCH
}
```

```rust
pub enum WriteOp {
    CreateNode { labels: Vec<SmolStr>, props: Expr, bind: Option<VarId> },
    CreateRel { from: VarId, to: VarId, rel_type: SmolStr, props: Expr, bind: Option<VarId> },
    MergeNode { labels: Vec<SmolStr>, props: Expr, on_create: Vec<WriteOp>, on_match: Vec<WriteOp>, bind: Option<VarId> },
    MergeRel { /* analogous */ },
    SetProperty { target: VarId, prop: SmolStr, value: Expr },
    SetLabels { target: VarId, labels: Vec<SmolStr> },
    RemoveProperty { target: VarId, prop: SmolStr },
    RemoveLabels { target: VarId, labels: Vec<SmolStr> },
    Delete { targets: Vec<Expr>, detach: bool },
}
```

### 12.2. Expression IR

A shared `Expr` enum reused by read and write plans. Fully resolved: every
variable reference carries its `VarId`; every function call is a
resolved `FunctionId`; every literal has an explicit type.

### 12.3. Ownership of identifiers

`VarId` is plan-scoped (not HIR-scoped) so a plan can outlive the HIR it
was lowered from. The lowering step produces a `VarMap` that consumers can
use to debug back to the source.

### 12.4. Parameters

Parameters surface in `Expr::Param { name: SmolStr, ty: Type }` with types
inferred from use-sites (see §7.6). A consumer binds parameters at
execution time; the plan does not carry values.

### 12.5. Consumer contract

Consumers implement an executor trait (not defined in this crate; lives in
the consumer's own code — this is the entire point of §2). A reference
`mock-executor` lives in `cypher-testkit` for integration testing of plan
lowering.

---

## 13. Formatter (`cypher-fmt`)

### 13.1. CST-driven, not AST-driven

The formatter walks the CST, not the AST. This preserves every byte of
trivia (comments, blank lines) and survives partial/broken input. The
output is a fresh string; the formatter does not mutate the tree.

### 13.2. Invariants

I13.1. **Idempotence.** `fmt(fmt(s)) == fmt(s)` for all syntactically valid
`s`. Asserted in property tests.
I13.2. **Semantic preservation.** `parse(fmt(s)).ast() == parse(s).ast()`
for all valid `s`. Asserted in property tests.
I13.3. **Trivia preservation.** Comments survive formatting, attached to
the nearest syntactic anchor. Blank lines are preserved modulo the "at most
one consecutive blank line" rule.

### 13.3. Options

- `width: usize` — soft line limit (default 100).
- `keyword_casing: {Upper, Lower, Preserve}` (default Upper).
- `trailing_commas: {Always, AsNeeded, Never}` (default AsNeeded).
- `indent: {Spaces(usize), Tabs}` (default 2 spaces).

All options are stable; adding options in future versions is allowed,
removing or renaming is a breaking change subject to §18.

### 13.4. Disabling

A magic comment `// cypher-fmt: off` / `// cypher-fmt: on` disables
formatting in a range. This is test-suite-asserted.

---

## 14. LSP Server (`cypher-lsp`)

### 14.1. Transport

Stdio (default) and TCP. Protocol: LSP 3.17. Implementation uses
`lsp-server` + `lsp-types` (the rust-analyzer stack).

### 14.2. Capabilities

- `textDocument/didOpen`, `didChange`, `didClose`, `didSave`
- `textDocument/publishDiagnostics` (push)
- `textDocument/completion` + `completionItem/resolve`
- `textDocument/hover`
- `textDocument/signatureHelp`
- `textDocument/definition`
- `textDocument/references`
- `textDocument/rename` + `prepareRename`
- `textDocument/formatting` + `rangeFormatting`
- `textDocument/codeAction` (surfaces fix-its)
- `textDocument/semanticTokens/full` + `/range`
- `textDocument/inlayHint` (types on WITH/UNWIND-bound vars)
- `textDocument/foldingRange`
- `workspace/executeCommand` for custom commands (explain-plan, lower-to-hir)

### 14.3. Schema configuration

The server exposes initialization options:

```json
{
  "schemaSource": "none" | "file" | "command",
  "schemaPath": "...",
  "schemaCommand": "...",
  "dialect": "GqlAligned" | "OpenCypherV9"
}
```

When `schemaSource` is `file` or `command`, the server invokes a small
adapter that reads the schema (via a pluggable `DynSchemaProvider` impl).
The adapter format is documented separately; generic-enough to accept JSON
schemas, TOML schemas, or a subprocess that emits JSON.

### 14.4. Performance

Completion latency target: p95 ≤ 25 ms on a 1,000-line workspace with
schema loaded, measured on a reference M-series Mac. Part of the CI
benchmark suite (§17.10).

---

## 15. Agent JSON API (`cypher-agent`)

### 15.1. Wire protocol

One request per line on stdin, one response per line on stdout. Each line
is a UTF-8 JSON object. Protocol version is carried in every request.

### 15.2. Operations

| Operation      | Input                                                       | Output                                           |
| -------------- | ----------------------------------------------------------- | ------------------------------------------------ |
| `parse`        | `{text, dialect}`                                           | `{cst_json, syntax_errors}`                      |
| `check`        | `{text, dialect, schema_digest?}`                           | `{diagnostics}`                                  |
| `complete`     | `{text, offset, dialect, schema_digest?}`                   | `{items: [...]}`                                 |
| `hover`        | `{text, offset, dialect, schema_digest?}`                   | `{markdown, range}`                              |
| `format`       | `{text, options}`                                           | `{formatted}` or `{diagnostics}`                 |
| `rewrite`      | `{text, fix_ids: [...]}`                                    | `{applied_edits, resulting_text}`                |
| `plan`         | `{text, dialect, schema_digest?}`                           | `{plan_json}` or `{diagnostics}`                 |
| `explain`      | `{text, dialect, schema_digest?}`                           | `{markdown}`                                     |
| `schema_set`   | `{schema_json}`                                             | `{schema_digest}`                                |
| `schema_clear` | `{}`                                                        | `{}`                                             |
| `shutdown`     | `{}`                                                        | `{}`                                             |

### 15.3. Semantics

- All operations are synchronous: one line in, one line out.
- Errors in the request itself (bad JSON, unknown op) surface as `{error:
  {code, message}}` and never crash the server.
- Schemas supplied via `schema_set` live in-process for the session. The
  agent never touches the filesystem unless explicitly told to.
- The agent is sandbox-safe: no network, no subprocess, no FS writes in v1.

### 15.4. Streaming

Not in v1. A single `plan` or `check` call is bounded and fast enough that
streaming is unnecessary. Streaming can be added by a future spec if a
consumer needs partial diagnostic delivery.

---

## 16. CLI (`cypher-cli`)

Binary name: `cypher`.

Subcommands:

- `cypher parse [--json|--debug] <file>` — parse and dump.
- `cypher check [--schema <path>] [--dialect ...] <file>` — run full analysis.
- `cypher fmt [--check] [-i|--in-place] <file>...` — format.
- `cypher plan [--schema <path>] <file>` — lower and print plan.
- `cypher explain [--schema <path>] <file>` — human-readable explanation.
- `cypher version`, `cypher help`.

Exit codes: `0` success, `1` diagnostics-present, `2` usage, `3` internal.

---

## 17. Testing & Correctness (rust-compiler-grade)

The non-negotiable heart of the spec. All subsections are invariants, not
aspirations. CI enforces each one.

### 17.1. Unit tests (`cargo test`)

Every crate has unit tests covering its public API. Internal modules are
tested at their own level (no integration-only smoke-test substitute).
Coverage gate (§17.9) applies per-crate.

### 17.2. Snapshot tests (`insta`)

- `cypher-syntax`: CST snapshots for every grammar production, both
  well-formed and error cases.
- `cypher-ast`: AST pretty-print snapshots.
- `cypher-hir`: HIR pretty-print snapshots with resolved-name overlay.
- `cypher-sema`: diagnostic snapshots (rendered).
- `cypher-plan`: plan-pretty snapshots.
- `cypher-fmt`: formatter input/output pairs.

Snapshot corpus lives in `crates/<name>/tests/snapshots/`. Regeneration is
a developer action (`cargo insta review`); CI rejects any unreviewed
change.

### 17.3. Property tests (`proptest`)

Minimum property suite:

P17.3.1. Parser totality: for any UTF-8 byte sequence, `parse` returns
without panic in bounded time and produces a CST whose text equals the
input. (10⁶ cases; CI time-boxed via `proptest` config.)

P17.3.2. CST losslessness: for any input `s`, `parse(s).syntax().to_string() == s`.

P17.3.3. Formatter idempotence: for any valid `s`,
`fmt(fmt(s)) == fmt(s)`.

P17.3.4. Formatter semantic preservation: for any valid `s`,
`parse(fmt(s)).ast().structurally_eq(&parse(s).ast())`.

P17.3.5. Diagnostic stability: permuting whitespace and comments in a
program does not change the set of non-trivia-sensitive diagnostics
produced.

P17.3.6. HIR round-trip: for any valid `s`, lowering is idempotent in the
sense that re-lowering the same AST produces a structurally-identical HIR.

P17.3.7. Plan round-trip: for any valid `s` with schema `S`, lowering is
deterministic — two calls produce byte-identical `plan_json` output.

### 17.4. Fuzzing (`cargo-fuzz` + `libfuzzer-sys`)

Fuzz targets:

- `fuzz_lexer` — input: arbitrary bytes; oracle: no panic, bounded time.
- `fuzz_parser` — input: arbitrary UTF-8; oracle: no panic, tree is
  lossless, bounded memory.
- `fuzz_formatter` — input: valid CST (generated via corpus); oracle:
  idempotence and semantic preservation.
- `fuzz_sema` — input: valid AST + schema; oracle: no panic, diagnostic
  spans within input range, bounded time.
- `fuzz_plan` — input: valid HIR + schema; oracle: no panic, bounded time.

Corpus: seeded from the TCK, the golden test directory, and a generative
cypher-grammar-aware input producer (written as a developer-only tool).

CI gate: `cargo fuzz run` for 5 minutes per target on every PR; 24h nightly
full run. Any new panic on the nightly corpus is a blocking bug.

Sanitizers: fuzz binaries built with ASan + UBSan. Found UB — even in
dependencies — is a blocking bug.

### 17.5. openCypher TCK conformance (`cypher-tck`)

The openCypher TCK is executed on every PR. We track conformance per
feature tag; a green tag means every scenario under that tag passes.

v1 must green-tag: `@MATCH`, `@OPTIONAL-MATCH`, `@WHERE`, `@RETURN`,
`@WITH`, `@UNWIND`, `@CREATE`, `@MERGE`, `@SET`, `@REMOVE`, `@DELETE`,
`@EXPRESSIONS` (minus subquery-related), `@AGGREGATIONS`, `@STRINGS`,
`@LISTS`, `@MAPS`, `@PATTERNS`, `@NULL`.

v1 must not green-tag: `@CALL-SUBQUERY`, `@EXISTS-SUBQUERY`, `@LOAD-CSV`.
Attempting them fails with the planned compatibility diagnostic codes
from §10.2.

### 17.6. Golden compiletest corpus (`cypher-testkit`)

UI-test-style golden tests modelled on `rustc`'s `compiletest`. Each test
is an input `.cypher` file paired with expected `.stderr` (diagnostic
output) and optionally expected `.plan.json`, `.ast.txt`, `.hir.txt`. A
custom runner compares by byte.

- `tests/ui/syntax/*` — parser error recovery & diagnostics.
- `tests/ui/sema/*` — semantic analysis diagnostics.
- `tests/ui/schema/*` — schema-aware checks; each test ships its own
  `.schema.json`.
- `tests/ui/dialect/*` — dialect mode differences.
- `tests/ui/plan/*` — plan lowering.
- `tests/ui/fmt/*` — formatter round-trips.

Regen is `cargo xtask bless`; CI rejects unblessed diffs.

### 17.7. Differential testing

Optional but recommended: for inputs in a shared corpus, run our parser
and `libcypher-parser` (via an FFI shim) and compare AST shape at a
structural level (ignoring their representation quirks). Divergences are
reported as notes, not failures; used to flag grammar drift.

Not a blocking gate — `libcypher-parser` has its own quirks and is not the
oracle. Enabled under `cfg(feature = "diff-test")`.

### 17.8. Mutation testing (`cargo-mutants`)

Run weekly on CI. Target ≥ 90% kill rate on `cypher-sema` and `cypher-hir`;
≥ 85% on `cypher-syntax`; ≥ 80% on `cypher-plan`. Surviving mutants are
triaged; each is either converted into a test or explicitly annotated as
"equivalent mutant" with justification.

### 17.9. Coverage (`cargo-llvm-cov`)

Per-crate line coverage minimums:

| Crate            | Minimum |
| ---------------- | ------- |
| `cypher-syntax`  | 90%     |
| `cypher-ast`     | 90%     |
| `cypher-hir`     | 90%     |
| `cypher-sema`    | 95%     |
| `cypher-schema`  | 95%     |
| `cypher-diag`    | 90%     |
| `cypher-plan`    | 90%     |
| `cypher-fmt`     | 95%     |
| `cypher-db`      | 85%     |
| `cypher-lsp`     | 75%     |
| `cypher-agent`   | 85%     |
| `cypher-cli`     | 80%     |

Lower on binaries because their hot path is integration-tested (§17.11);
higher on semantic libraries because those are the correctness surface.

### 17.10. Benchmarks (`criterion`) with regression gates

- `bench_parse` — 1 KiB, 10 KiB, 100 KiB, 1 MiB realistic corpora.
- `bench_sema` — same sizes, with and without schema.
- `bench_plan` — same.
- `bench_fmt` — same.
- `bench_lsp_completion` — simulated LSP completion workload.
- `bench_incremental` — 10,000-line file, sequence of small edits; measure
  per-edit reanalysis cost. Includes a long-horizon workload (10k edits,
  with both LSP FileId-churn and agent single-FileId access patterns) that
  asserts the §11.6 steady-state-RSS bound (within ±10% of first-1k-edits
  baseline). RSS drift ≥ 10% is a blocking regression.

Results are stored in the repo (`bench/baseline/`) and a CI job posts
regressions ≥ 10% as blocking. Thresholds per-benchmark, documented in
`bench/README.md`.

### 17.11. Integration tests

- LSP: a test harness spins up the binary, drives it via `lsp-types` JSON-
  RPC, and asserts responses. Covers: open-edit-diagnose, completion,
  hover, rename, format, code-actions-apply.
- Agent: a test harness pipes JSON requests to the binary, asserts JSON
  responses byte-for-byte where deterministic, structurally otherwise.
- CLI: `assert_cmd` + `predicates` tests for every subcommand, golden
  stdout/stderr.

### 17.12. Miri

Run on all library crates in CI nightly. Fails on any UB (including in
dependencies — we use `MIRIFLAGS=-Zmiri-strict-provenance`).

### 17.13. `unsafe_code` policy

`#![forbid(unsafe_code)]` at the workspace level. A dependency that uses
`unsafe` is acceptable (most of the ecosystem does); our first-party code
does not.

### 17.14. Determinism

Every public output (CST, AST text, HIR text, diagnostics, plan, formatted
text) must be deterministic for a given input + dialect + schema_digest.
No `HashMap` iteration order leaks into outputs; use `BTreeMap` or
`IndexMap` in output-facing code. Property P17.3.7 asserts this.

### 17.15. CI matrix

- Rust: stable, beta, nightly.
- OS: Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64).
- Feature matrix: default, `no-default-features`, `lsp`-only, `fmt`-only,
  `schema`-only. Not all crates ship feature flags; only the meta-crate
  and binaries do.

Clippy at `-D warnings`; rustdoc with `-D warnings`.

### 17.16. Security

- `cargo audit` on every PR; any unresolved advisory blocks merge.
- `cargo deny` with an allowlist for licenses.
- Dependency pinning via `Cargo.lock` committed.
- No network access in tests (enforced by the test harness where
  possible).

### 17.17. Release gating

A release is permitted only if all of: unit tests green, snapshot corpus
unchanged without review, TCK green for v1 scope, fuzz clean for 24h
nightly, coverage thresholds met, benchmarks within gates, Miri clean,
mutation kill rate met, `cargo audit` clean, `cargo xtask check-recovery`
clean. Release workflow is `cargo xtask release`.

### 17.18. Grammar-recovery structural check (`cargo xtask check-recovery`)

Enforces the §4.3 coverage invariant. The subcommand:

1. Parses `cypher-ast/cypher.ungrammar` into its production set.
2. Parses `cypher-syntax/docs/recovery.md` into its entry set (entries are
   keyed by production name in stable H3 headings).
3. Fails with a two-sided diff if either set contains names not in the
   other.

Runs on every PR as a blocking gate. Intended to be cheap (milliseconds),
so it runs unconditionally, not matrix-gated.

---

## 18. Release, Versioning, MSRV

- Semantic versioning. `0.x` until the library API is judged stable;
  after 1.0 every breaking change is a major bump.
- MSRV: pinned in `rust-toolchain.toml`. v1 MSRV target: `1.94` (matches
  the trench workspace; consumers are not penalised).
- Public API surface minimised: anything not re-exported from the meta-
  crate `cypher` or from a binary is considered internal and may break in
  minor releases.
- Changelog: `CHANGELOG.md` per crate, keep-a-changelog format.

---

## 19. v1 Scope Matrix

| Construct                             | v1     | Notes                                    |
| ------------------------------------- | ------ | ---------------------------------------- |
| `MATCH`                               | ✅     |                                          |
| `OPTIONAL MATCH`                      | ✅     |                                          |
| Pattern: nodes                        | ✅     |                                          |
| Pattern: relationships (fixed-length) | ✅     |                                          |
| Pattern: variable-length `*m..n`      | ✅     |                                          |
| Pattern: shortest-path functions      | ✅     | `shortestPath`, `allShortestPaths`       |
| Pattern: relationship type disjunction | ✅    | `-[:A\|B]->`                             |
| `WHERE`                               | ✅     |                                          |
| `RETURN` / `DISTINCT` / `*`           | ✅     |                                          |
| `ORDER BY` / `SKIP` / `LIMIT`         | ✅     |                                          |
| `WITH` (scope barrier)                | ✅     |                                          |
| `UNWIND`                              | ✅     |                                          |
| `CREATE`                              | ✅     |                                          |
| `MERGE` + `ON CREATE`/`ON MATCH SET`  | ✅     |                                          |
| `SET` properties, labels, map         | ✅     |                                          |
| `REMOVE`                              | ✅     |                                          |
| `DELETE` / `DETACH DELETE`            | ✅     |                                          |
| `CALL <proc>` (standalone + YIELD)    | ✅     | Declared via `SchemaProvider::procedure` |
| `CALL { <subquery> }`                 | ❌     | Deferred                                 |
| `EXISTS { <subquery> }`               | ❌     | Deferred                                 |
| `COUNT { ... }` expressions           | ❌     | Deferred                                 |
| `LOAD CSV`                            | ❌     | Deferred                                 |
| `SHOW …`, `USE …`, `CYPHER 5`         | ❌     | Deferred                                 |
| `UNION` / `UNION ALL`                 | ✅     |                                          |
| List comprehensions                   | ✅     | Desugared in HIR                         |
| Map projection                        | ✅     | Desugared in HIR                         |
| Pattern predicates in expressions     | ✅     | Desugared in HIR                         |
| CASE                                  | ✅     |                                          |
| Temporal: `date`, `datetime`          | ✅     |                                          |
| Temporal: `time`, `localdatetime`, …  | ❌     | Deferred                                 |
| Spatial: `point`                      | ❌     | Deferred                                 |
| Dialect: `GqlAligned`                 | ✅     | Canonical                                |
| Dialect: `OpenCypherV9`               | ✅     | Compat                                   |
| Dialect: `Neo4jCurrent`               | ❌     | Deferred                                 |

---

## 20. Deferred / Roadmap

- D1. `CALL { ... }` and `EXISTS { ... }` subqueries (needs full scope graph
  with nested sub-scopes and existential semantics).
- D2. Neo4j-current dialect mode.
- D3. Temporal types beyond `date`/`datetime`.
- D4. Spatial types.
- D5. Cost-based plan optimizer. Not a v1 concern; consumers own cost.
- D6. Multi-file scope / import system. Cypher has no imports; if any
  dialect introduces them, spec separately.
- D7. SARIF diagnostics backend.
- D8. Streaming agent-API responses.

Each deferred item is a separate spec when taken up (0002, 0003, …).

---

## 21. Open Questions

Q21.1. Should we ship a reference mock executor for `Plan` in a separate
crate (`cypher-mock-executor`) or only in `cypher-testkit`? Default: in
testkit (we don't want people shipping our mock to prod). Flag for
reconsideration after the Plan IR is stable.

Q21.2. `StandardLibrary` is our canonical function catalog, but openCypher
functions have subtle return-type rules that depend on argument types
(e.g., `coalesce` returns the narrowest supertype). Do we model this via a
signature-level function in Rust (`fn infer_return(&self, args: &[Type]) ->
Type`) or via a small type-function language? Default: Rust closures on
`FunctionSignature`. Reconsider if expressiveness is lacking.

Q21.3. Should error recovery emit multiple "candidate parses" for
ambiguous syntax errors, or commit to one? Default: one. LSP quality does
not benefit from multiple candidates in practice.

Q21.4. Do we expose a "lowered Cypher" text rendering (pretty-print HIR
back to Cypher)? Useful for debugging; invites misuse as a "canonical
form." Default: yes, behind a `debug_` prefix, not public API.

Q21.5. Should the formatter have a "minimal" mode (no option-driven
choices, always produce the same output for a given parsed query)?
Default: no; the formatter is opinionated but option-driven. Flag for
revisit if consumers want canonicalisation.

---

## 22. Glossary

- **CST** — Concrete Syntax Tree. Lossless, includes trivia.
- **AST** — Abstract Syntax Tree. Typed wrappers over CST; still lossless
  in that the CST is reachable.
- **HIR** — High-level Intermediate Representation. Owned, resolved,
  desugared. The analysis target.
- **Plan IR** — Logical query plan. Consumer-facing output of lowering.
- **`SyntaxKind`** — rowan-compatible enumeration of every node and token
  kind.
- **Dialect mode** — a parse-time and check-time switch selecting
  GqlAligned or OpenCypherV9 semantics.
- **Diagnostic code** — a stable `EnnnnN` / `WnnnnN` / `NnnnnN` string
  identifying a specific check; see §10.2.
- **Schema digest** — a content-addressed identity of a schema snapshot
  used as a Salsa input.

---

## 23. Non-normative notes

N23.1. rust-analyzer and Biome are the nearest prior art for the CST + AST
+ HIR + Salsa architecture; both are worth rereading before touching
§4–§11 code. Nothing in this spec conflicts with their conventions; where
this spec is silent, defer to their practice.

N23.2. The Neo4j `cypher-language-support` monorepo is not a design
source for this spec — it is a product comparator. If an LSP behaviour
here diverges from theirs, it is intentional.

N23.3. `libcypher-parser` is the most battle-tested Cypher parser today.
We do not depend on it, but its test corpus and AST shape are worth
mining.

---

*End of spec 0001.*
