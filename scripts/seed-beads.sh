#!/usr/bin/env bash
# Seed the cypher/ beads workspace from docs/bead-seed.md.
#
# Idempotent-ish: safe to run against an empty .beads/. Re-running against
# a populated .beads/ duplicates beads — clear .beads/ first or add your own
# guards if needed.
#
# Every bead cites the spec section(s) it implements. Acceptance criteria
# live in spec 0001, not duplicated here.

set -euo pipefail
cd "$(dirname "$0")/.."   # cypher/

if [ ! -f .beads/beads.db ]; then
  br init --prefix cy
fi

# --- helpers --------------------------------------------------------------

mk() {  # mk <title> <priority> <labels> <description>  → echoes ID
  br create "$1" -t task -p "$2" -l "$3" -d "$4" --silent
}

dep() {  # dep <child_id> <parent_id>
  br dep add "$1" "$2" -q >/dev/null
}

S() { printf 'spec §%s. See docs/specs/0001-cypher-frontend.md.\n\n%s' "$1" "$2"; }

# --- Tier 0: foundation ---------------------------------------------------

b001=$(mk "Workspace hygiene + forbid(unsafe)"               P0 "tier-0,layer:infra" "$(S '2.C4, 17.13, 17.16' 'Audit Cargo.toml lints, #![forbid(unsafe_code)] everywhere, .gitignore, clippy.toml, deny.toml, published-shaped repo.')")
b002=$(mk "xtask skeleton (gate/bless/codegen/release/fuzz)" P0 "tier-0,layer:infra,crate:xtask" "$(S '11' 'Stub subcommands. gate runs clippy+fmt+test+deny; bless regenerates UI tests; codegen regenerates AST from ungrammar; release is gated; fuzz wraps cargo fuzz. Stubs ok.')")
b003=$(mk "Pre-commit hook → cargo xtask gate"               P0 "tier-0,layer:infra" "$(S '4.3' 'Install .git/hooks/pre-commit that calls cargo xtask gate. Document the install step in the root README.')")
b004=$(mk "Non-coupling CI lint (denylist enforcement)"      P0 "tier-0,layer:infra" "$(S '2.C1, 2.C2' 'CI step that greps for intel-/trench- deps in any Cargo.toml and for denylisted words (Actor, Event, Operation, Capability, provenance, branch, bitemporal, expertise) in any .rs file outside fixtures.')")
b005=$(mk "CI matrix bootstrap (stable/beta/nightly × 3 OS)" P1 "tier-0,layer:infra" "$(S '17.15' 'GitHub Actions workflow matrix: rust={stable,beta,nightly} × os={ubuntu,macos,windows}. clippy -D warnings, rustdoc -D warnings.')")
b006=$(mk "Diagnostic code registry scaffolding"             P0 "tier-0,layer:diag,crate:cypher-diag" "$(S '10.2' 'Build the DiagCode registry at crates/cypher-diag/src/codes.rs. Stable codes, no reuse, dupes forbidden. CI asserts: every emit references a registered const; every registered const is emitted.')")

# --- Tier 1: syntax -------------------------------------------------------

b010=$(mk "SyntaxKind enum (hand-enumerated, non-exhaustive)" P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.1, 4.4' 'Canonical grammar reference. Covers every node kind and every token kind. Non-exhaustive per rowan pattern.')")
b011=$(mk "Logos lexer — all token classes"                   P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.1' 'All token classes: keywords (case-insensitive, preserve case), idents, quoted idents, strings (full escape set), numbers (dec/hex/oct/bin + float), params, punctuation, comments, whitespace. Error token carries offending range. Trivia attached leading/trailing.')")
b012=$(mk "Parser event stream + Pratt precedence"            P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.2' 'Hand-written event-based recursive-descent. Events: Start(kind)/Token/Finish/Error. Pratt for expressions. Separation of events from tree build.')")
b013=$(mk "Rowan green/red tree builder"                      P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.4' 'Builder consumes the parser event stream and produces a rowan green tree. CST is lossless: cst.syntax().to_string() == input.')")
b014=$(mk "Error recovery table (recovery.md)"                P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.3' 'Normative per-production recovery table at crates/cypher-syntax/docs/recovery.md. Sync sets, skip-and-recover tokens, virtual-insertion rules.')")
b015=$(mk "LineIndex + UTF-8 TextRange helpers"               P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.5' 'Byte-offset TextRange throughout. LineIndex converts to line/col for rendering. UTF-16 conversion is LSP-boundary-only.')")
b016=$(mk "Statement boundary splitting"                      P2 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '4.6' 'Multi-statement files via SyntaxKind::Statement children. Trailing ; optional. Empty file is empty tree, not error.')")
b017=$(mk "Syntax snapshot corpus bootstrap (insta)"          P1 "tier-1,layer:syntax,crate:cypher-syntax" "$(S '17.2' 'CST snapshots for every grammar production, well-formed and error cases. Under crates/cypher-syntax/tests/snapshots/. cargo insta review is the review loop.')")

# --- Tier 2: AST ----------------------------------------------------------

b020=$(mk "cypher.ungrammar description"                       P1 "tier-2,layer:ast,crate:cypher-ast" "$(S '5.2' 'Grammar description used by xtask codegen. Lives at crates/cypher-ast/cypher.ungrammar.')")
b021=$(mk "xtask codegen for AST wrappers"                     P1 "tier-2,layer:ast,crate:xtask" "$(S '5.2' 'Generates hundreds of wrapper types, field accessors, AstNode impls from cypher.ungrammar. Generated file is committed; CI verifies regen matches.')")
b022=$(mk "Typed AST enum wrappers for sum types"              P1 "tier-2,layer:ast,crate:cypher-ast" "$(S '5.3, 5.4' 'Enum wrappers for Expression / Clause / Pattern alternation. Missing fields return Option even if valid Cypher requires presence.')")
b023=$(mk "AST snapshot corpus"                                P1 "tier-2,layer:ast,crate:cypher-ast" "$(S '17.2' 'AST pretty-print snapshots.')")

# --- Tier 3: HIR ----------------------------------------------------------

b030=$(mk "HIR node types + HirId"                             P1 "tier-3,layer:hir,crate:cypher-hir" "$(S '6.1' 'Owned resolved representation. HirId is statement-scoped index. AST↔HIR map preserved.')")
b031=$(mk "AST → HIR lowering"                                 P1 "tier-3,layer:hir,crate:cypher-hir" "$(S '6.1' 'One pass per statement. Pattern, PatternElement, Expr, Projection, VarId.')")
b032=$(mk "Desugaring (list comp / map proj / pattern pred / shorthand)" P1 "tier-3,layer:hir,crate:cypher-hir" "$(S '6.1' 'List comprehensions → filter+map. Map projection → map construction. Pattern predicates → ExprKind::PatternPredicate. {name: $n} shorthand → explicit WHERE.')")
b033=$(mk "ScopeGraph + ResolvedNames + WITH barrier"          P1 "tier-3,layer:hir,crate:cypher-hir" "$(S '6.2' 'Per-statement scope stack. MATCH/CREATE/MERGE bind until WITH. WITH is barrier: only projected vars visible. UNWIND AS v / CALL YIELD bind in enclosing scope.')")
b034=$(mk "Variable kinds + kind-consistency"                  P1 "tier-3,layer:hir,crate:cypher-hir" "$(S '6.3' 'Node / Relationship / Path / Value. Kind mismatches reported at sema, not at name res.')")
b035=$(mk "HIR snapshot corpus with resolved-name overlay"     P1 "tier-3,layer:hir,crate:cypher-hir" "$(S '17.2' 'HIR pretty-print with resolved names annotated.')")

# --- Tier 4: Schema -------------------------------------------------------

b040=$(mk "SchemaProvider trait"                               P1 "tier-4,layer:schema,crate:cypher-schema" "$(S '8.1' 'labels/rel types/properties/endpoints/inverse_of/function/procedure/schema_digest. Send + Sync + static.')")
b041=$(mk "Schema supporting types"                            P1 "tier-4,layer:schema,crate:cypher-schema" "$(S '8.2' 'PropertyDecl, PropertyType (incl. Opaque), EndpointDecl, FunctionSignature, ProcedureSignature, Cardinality, ProcMode.')")
b042=$(mk "StandardLibrary catalog impl"                       P1 "tier-4,layer:schema,crate:cypher-schema" "$(S '8.3' 'openCypher-standard functions. Wraps any SchemaProvider via StandardLibrary::wrap(inner).')")
b043=$(mk "schema_digest stability + identity tests"           P1 "tier-4,layer:schema,crate:cypher-schema" "$(S '8.5' 'Digest changes on any observable-surface change; stable across identical schemas. Used as Salsa input.')")

# --- Tier 5: Sema ---------------------------------------------------------

b050=$(mk "cypher-sema skeleton + Type enum"                   P1 "tier-5,layer:sema,crate:cypher-sema" "$(S '7.2' 'Type = Any | Null | Bool | Int | Float | Num | String | Date | Datetime | List | Map | Node | Relationship | Path | Union | Unknown.')")
b051=$(mk "Unification engine"                                 P1 "tier-5,layer:sema,crate:cypher-sema" "$(S '7.2' 'Small unification engine. Num unifies to Int or Float. Any is universal subtype. Int/Int = Int per openCypher (no promotion).')")
b052=$(mk "Schema-free pass"                                   P1 "tier-5,layer:sema,crate:cypher-sema" "$(S '7.1, 7.3, 7.4, 7.6' 'Aggregation scope (RETURN/WITH/ORDER BY), clause ordering, kind mismatches, DISTINCT rules, parameter type discipline, structural type errors.')")
b053=$(mk "Schema-aware pass"                                  P1 "tier-5,layer:sema,crate:cypher-sema" "$(S '7.1' 'Unknown labels / rel types / properties / fn arity + types / procedure arity / endpoint mismatches.')")
b054=$(mk "DialectGate plumbing"                               P1 "tier-5,layer:sema,crate:cypher-sema" "$(S '9' 'Every dialect-divergent construct flows through a named DialectGate with its own E4xxx code. No scattered if dialect == … checks.')")
b055=$(mk "Sema snapshot corpus + compiletest"                 P1 "tier-5,layer:sema,crate:cypher-sema" "$(S '17.2, 17.6' 'Rendered diagnostic snapshots + tests/ui/sema/**. bless via cargo xtask bless.')")

# --- Tier 6: Diagnostics --------------------------------------------------

b060=$(mk "Diagnostic / Severity / Label / FixIt"              P1 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.1' 'Struct with code, severity, message, primary+secondary labels, notes, related info, fix-its with Applicability.')")
b061=$(mk "Codes E0001–E0999 (syntax)"                         P1 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.2' 'Lexer + parser error codes. Registered with messages and docs page stubs.')")
b062=$(mk "Codes E1000–E5999 (res / sema / dialect / type)"    P1 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.2' 'Name resolution, schema-free sema, schema-aware sema, dialect, type system.')")
b063=$(mk "Codes W6000–N8999 (lint / perf / note)"             P2 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.2' 'Style/lint, performance, informational.')")
b064=$(mk "Plain-text renderer via codespan-reporting"         P1 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.3' 'Terminal with ANSI, uses codespan-reporting under the hood.')")
b065=$(mk "JSON renderer (agent API)"                          P1 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.3' 'Stable field names. One object per diagnostic.')")
b066=$(mk "LSP diagnostic converter"                           P1 "tier-6,layer:diag,crate:cypher-diag" "$(S '10.3' 'Diagnostic → lsp_types::Diagnostic small converter.')")

# --- Tier 7: Plan IR ------------------------------------------------------

b070=$(mk "ReadOp + WriteOp + Expr enums"                      P1 "tier-7,layer:plan,crate:cypher-plan" "$(S '12.1' 'Source/Expand/Filter/Project/Aggregate/OrderBy/Skip/Limit/Distinct/Unwind/Union/With/OptionalJoin. Write: Create*/Merge*/Set*/Remove*/Delete.')")
b071=$(mk "HIR → Plan lowering"                                P1 "tier-7,layer:plan,crate:cypher-plan" "$(S '12.1' 'Lowering produces DAG of operators with typed column schemas. Plan-scoped VarIds + VarMap for debugging.')")
b072=$(mk "Plan serde JSON + pretty-print"                     P1 "tier-7,layer:plan,crate:cypher-plan" "$(S '12' 'Stable JSON for agent API + pretty print for explain.')")
b073=$(mk "Plan snapshot corpus"                               P1 "tier-7,layer:plan,crate:cypher-plan" "$(S '17.2' 'Plan-pretty snapshots.')")

# --- Tier 8: Formatter ----------------------------------------------------

b080=$(mk "CST-driven formatter core"                          P1 "tier-8,layer:fmt,crate:cypher-fmt" "$(S '13.1' 'Walks CST, not AST. Preserves trivia. Tolerates partial input.')")
b081=$(mk "Formatter property tests (idempotence + sem pres)"  P1 "tier-8,layer:fmt,crate:cypher-fmt" "$(S '13.2, 17.3' 'fmt(fmt(s)) == fmt(s). parse(fmt(s)).ast() structurally equals parse(s).ast().')")
b082=$(mk "Formatter options + magic comment toggle"           P2 "tier-8,layer:fmt,crate:cypher-fmt" "$(S '13.3, 13.4' 'width/keyword_casing/trailing_commas/indent. // cypher-fmt: off/on ranges.')")
b083=$(mk "Formatter snapshot corpus"                          P1 "tier-8,layer:fmt,crate:cypher-fmt" "$(S '17.2' 'Input/output snapshot pairs.')")

# --- Tier 9: Incremental DB ----------------------------------------------

b090=$(mk "Salsa skeleton + Database"                          P1 "tier-9,layer:db,crate:cypher-db" "$(S '11.1' 'Salsa 2022-style. Database is Send + Sync via snapshots.')")
b091=$(mk "Input queries (source/dialect/digest/opts)"         P1 "tier-9,layer:db,crate:cypher-db" "$(S '11.2' 'source_text, dialect, schema_digest, semantic_options.')")
b092=$(mk "Derived queries (parse→plan pipeline)"              P1 "tier-9,layer:db,crate:cypher-db" "$(S '11.3' 'parse, ast, hir, sema, plan, diagnostics, formatted. Memoized, precisely invalidated.')")
b093=$(mk "Snapshot / concurrency / FileId model"              P1 "tier-9,layer:db,crate:cypher-db" "$(S '11.4, 11.5' 'FileId = caching unit. Workspace-scoped SchemaProvider. One Database + snapshot per request.')")

# --- Tier 10: Binaries ----------------------------------------------------

b100=$(mk "cypher-cli subcommands (parse/check/fmt/plan/explain)" P1 "tier-10,layer:bin,crate:cypher-cli" "$(S '16' 'Binary name cypher. Exit codes: 0 ok, 1 diagnostics, 2 usage, 3 internal.')")
b101=$(mk "cypher-lsp stdio + LSP capabilities"                P1 "tier-10,layer:bin,crate:cypher-lsp" "$(S '14' 'LSP 3.17 via lsp-server + lsp-types. Full capability set (§14.2). Init options select schema source + dialect.')")
b102=$(mk "cypher-agent JSON protocol (11 ops)"                P1 "tier-10,layer:bin,crate:cypher-agent" "$(S '15' 'parse/check/complete/hover/format/rewrite/plan/explain/schema_set/schema_clear/shutdown. Sandbox-safe: no network/FS/subprocess.')")
b103=$(mk "cypher-tck harness + v1 green tags"                 P1 "tier-10,layer:bin,crate:cypher-tck" "$(S '17.5' 'v1 must green-tag MATCH/OPTIONAL-MATCH/WHERE/RETURN/WITH/UNWIND/CREATE/MERGE/SET/REMOVE/DELETE/EXPRESSIONS/AGGREGATIONS/STRINGS/LISTS/MAPS/PATTERNS/NULL; must not green-tag CALL-SUBQUERY/EXISTS-SUBQUERY/LOAD-CSV.')")

# --- Tier 11: Big-picture quality ----------------------------------------

b110=$(mk "Property suite P17.3.1–P17.3.7"                     P2 "tier-11,layer:infra" "$(S '17.3' 'Parser totality, CST losslessness, fmt idempotence, fmt sem pres, diag stability under trivia, HIR round-trip, plan determinism.')")
b111=$(mk "Fuzz targets + 5-min PR gate + 24h nightly"         P2 "tier-11,layer:infra" "$(S '17.4' 'fuzz_lexer/parser/formatter/sema/plan. ASan+UBSan. New panics on nightly corpus are blocking bugs.')")
b112=$(mk "Compiletest corpus + cargo xtask bless"             P2 "tier-11,layer:infra,crate:cypher-testkit" "$(S '17.6' 'UI-test-style golden tests under tests/ui/{syntax,sema,schema,dialect,plan,fmt}/**. Byte-exact stderr comparison.')")
b113=$(mk "Criterion benches + 10% regression gate"            P2 "tier-11,layer:infra" "$(S '17.10' 'bench_parse/sema/plan/fmt/lsp_completion/incremental. Baseline under bench/baseline/.')")
b114=$(mk "cargo-mutants config + weekly CI + kill rates"      P2 "tier-11,layer:infra" "$(S '17.8' '≥90% sema, ≥90% hir, ≥85% syntax, ≥80% plan. Survivors triaged into tests or equivalent-mutant annotations.')")
b115=$(mk "Full CI matrix"                                     P2 "tier-11,layer:infra" "$(S '17.15' 'stable/beta/nightly × linux(x86,arm)/macos(x86,arm)/windows(x86). Feature matrix: default, no-default-features, lsp-only, fmt-only, schema-only.')")
b116=$(mk "Miri CI (strict-provenance)"                        P2 "tier-11,layer:infra" "$(S '17.12' 'MIRIFLAGS=-Zmiri-strict-provenance. UB in deps is a blocking bug.')")
b117=$(mk "cargo deny allowlist + cargo audit PR gate"         P2 "tier-11,layer:infra" "$(S '17.16' 'License allowlist, advisory blocking, Cargo.lock committed.')")
b118=$(mk "llvm-cov coverage gates per crate"                  P2 "tier-11,layer:infra" "$(S '17.9' 'Per-crate floors in §17.9 table. Block merges below floor.')")
b119=$(mk "cypher-testkit dev crate"                           P2 "tier-11,layer:infra,crate:cypher-testkit" "$(S '3.1, 17.6' 'Fixtures, snapshot helpers, compiletest runner. Dev-only, not re-exported from meta crate cypher.')")

# --- Dependency edges -----------------------------------------------------

# Tier 1
dep "$b010" "$b001"
dep "$b011" "$b010"; dep "$b012" "$b010"; dep "$b013" "$b010"; dep "$b013" "$b012"
dep "$b014" "$b012"; dep "$b015" "$b001"; dep "$b016" "$b013"; dep "$b017" "$b013"

# Tier 2
dep "$b020" "$b010"; dep "$b021" "$b002"; dep "$b021" "$b020"
dep "$b022" "$b021"; dep "$b023" "$b022"

# Tier 3
dep "$b030" "$b022"; dep "$b031" "$b030"; dep "$b032" "$b031"
dep "$b033" "$b031"; dep "$b034" "$b033"; dep "$b035" "$b032"; dep "$b035" "$b034"

# Tier 4
dep "$b040" "$b001"; dep "$b041" "$b040"; dep "$b042" "$b041"; dep "$b043" "$b040"

# Tier 5
dep "$b050" "$b035"; dep "$b051" "$b050"
dep "$b052" "$b051"; dep "$b052" "$b006"
dep "$b053" "$b052"; dep "$b053" "$b040"; dep "$b053" "$b041"; dep "$b053" "$b042"
dep "$b054" "$b052"
dep "$b055" "$b053"; dep "$b055" "$b054"

# Tier 6
dep "$b060" "$b006"
dep "$b061" "$b060"; dep "$b061" "$b017"
dep "$b062" "$b060"; dep "$b062" "$b055"
dep "$b063" "$b060"
dep "$b064" "$b060"; dep "$b065" "$b060"; dep "$b066" "$b060"

# Tier 7
dep "$b070" "$b035"; dep "$b071" "$b070"; dep "$b072" "$b071"; dep "$b073" "$b072"

# Tier 8
dep "$b080" "$b013"; dep "$b081" "$b080"; dep "$b082" "$b080"; dep "$b083" "$b080"

# Tier 9
dep "$b090" "$b055"; dep "$b090" "$b073"; dep "$b090" "$b066"; dep "$b090" "$b083"
dep "$b091" "$b090"; dep "$b092" "$b091"; dep "$b093" "$b092"

# Tier 10
dep "$b100" "$b093"
dep "$b101" "$b093"
dep "$b102" "$b093"; dep "$b102" "$b065"
dep "$b103" "$b093"

# Tier 11 — cross-cutting, loose deps
dep "$b110" "$b017"; dep "$b110" "$b023"; dep "$b110" "$b035"; dep "$b110" "$b055"
dep "$b110" "$b073"; dep "$b110" "$b083"
dep "$b111" "$b011"; dep "$b111" "$b013"; dep "$b111" "$b080"; dep "$b111" "$b053"; dep "$b111" "$b071"
dep "$b112" "$b002"; dep "$b112" "$b055"
dep "$b113" "$b017"; dep "$b113" "$b055"; dep "$b113" "$b073"; dep "$b113" "$b083"
dep "$b113" "$b100"; dep "$b113" "$b101"
dep "$b114" "$b055"; dep "$b114" "$b035"; dep "$b114" "$b017"; dep "$b114" "$b073"
dep "$b115" "$b005"; dep "$b116" "$b115"; dep "$b117" "$b001"; dep "$b118" "$b115"
dep "$b119" "$b002"

echo
echo "Seed complete. Summary:"
br stats 2>/dev/null || br status 2>/dev/null || true
echo
echo "Next: bv --robot-triage   (if bv installed)  or  br ready --json"
