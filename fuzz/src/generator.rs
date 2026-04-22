//! Grammar-aware random Cypher generator (bead cy-h07.1).
//!
//! Produces statements that parse cleanly — i.e. the resulting CST contains
//! zero `ERROR` nodes and `Parse::errors()` is empty. Used by
//! `fuzz_structured_parse` to raise fuzz quality from "random bytes" to
//! "valid Cypher with controlled structural variation" so libFuzzer can
//! explore deeper clauses faster.
//!
//! Properties relied on downstream:
//!
//! - **Deterministic given the seed.** The same `RngCore` seed produces the
//!   same output byte-for-byte, on every platform, forever. The generator
//!   picks via raw `next_u32() % N` rather than `gen_range` so a version
//!   bump of `rand` can't silently reshape the distribution.
//! - **Valid by construction.** Every reachable branch is a known-clean
//!   parse path. The generator does not emit recovery-bait (unbalanced
//!   delimiters, truncated clauses, stray keywords).
//! - **Depth-bounded.** Recursive productions (expressions, patterns) fall
//!   through to a leaf once `max_depth` hits zero. The walker itself cannot
//!   stack-overflow regardless of seed.
//!
//! Clause coverage (v0):
//!
//! - `MATCH <pattern> [WHERE <expr>]`
//! - `OPTIONAL MATCH <pattern>`
//! - `WITH <items> [WHERE <expr>]` (followed by a forced `RETURN` tail so
//!   the statement is well-formed)
//! - `UNWIND <expr> AS <var>`
//! - `CREATE (<node>)`
//! - `RETURN [DISTINCT] <items> [ORDER BY …] [SKIP n] [LIMIT n]`
//!
//! Expression coverage:
//!
//! - Literals (int, float, string, null, bool)
//! - Variables (from a fixed pool)
//! - Binary operators (`+ - * / AND OR =`)
//! - Function calls (`count`, `size`, `coalesce`, `toLower`, `toUpper`)
//! - List literals `[e, e, …]`
//! - Map literals `{k: e, …}`
//!
//! Pattern coverage:
//!
//! - `(n)`, `(n:Label)`, `(n:Label {k: v})`
//! - Optional relationship chain `(…) -[r:TYPE]-> (…)` with a random
//!   direction.
//!
//! The pool of identifier names is fixed so `RETURN n` always resolves to
//! a `MATCH (n)` introduced earlier in the same statement — keeping name
//! resolution happy is a *future* enhancement; v0 only asserts the parser
//! sees a clean tree.

use core::sync::atomic::{AtomicU64, Ordering};

use rand_core::RngCore;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate one syntactically valid Cypher statement using `rng`.
///
/// `max_depth` caps recursion on expression / pattern productions. A value
/// of `4`–`6` is a good default for fuzzing; `0` produces only the thinnest
/// leaves (literal-only RETURN).
///
/// Each call bumps the process-wide [`COVERAGE`] counters (spec §17.4,
/// bead cy-h07 acceptance: every clause covered at least once per 1k
/// iterations).
#[must_use]
pub fn random_valid_cypher<R: RngCore + ?Sized>(rng: &mut R, max_depth: u8) -> String {
    let mut g = Gen::new(rng, max_depth);
    g.statement();
    g.finish()
}

// ---------------------------------------------------------------------------
// Coverage tracking (cy-h07)
// ---------------------------------------------------------------------------

/// Clause / expression categories tracked by [`COVERAGE`].
///
/// Ordering is stable and forms the index into the parallel arrays on
/// [`CoverageCounters`]. New variants append to the end so old indices
/// keep meaning; this mirrors the `SyntaxKind` stability policy in
/// `crates/cypher-syntax/src/kind.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clause {
    /// `MATCH <pattern> [WHERE …]`
    Match,
    /// `OPTIONAL MATCH <pattern>`
    OptionalMatch,
    /// `WITH <items> [WHERE …]`
    With,
    /// `UNWIND <expr> AS <var>`
    Unwind,
    /// `CREATE (<node>)`
    Create,
    /// `RETURN …` — always emitted by `statement()` as the tail.
    Return,
    /// A binary-operator expression was emitted somewhere in the statement.
    ExprBinary,
    /// A function-call expression was emitted.
    ExprFunctionCall,
    /// A list literal `[…]` was emitted.
    ExprList,
    /// A map literal `{…}` was emitted.
    ExprMap,
    /// A property access `n.k` was emitted at the return-item or leaf level.
    ExprPropertyAccess,
    /// A relationship pattern was emitted inside a MATCH/OPTIONAL MATCH.
    RelPattern,
    /// `ORDER BY` tail on a RETURN was emitted.
    OrderBy,
    /// `SKIP` tail on a RETURN was emitted.
    Skip,
    /// `LIMIT` tail on a RETURN was emitted.
    Limit,
    /// `RETURN DISTINCT` was emitted.
    Distinct,
}

impl Clause {
    /// Dense index for array-backed counter storage.
    const fn as_index(self) -> usize {
        self as usize
    }

    /// Total number of variants. Kept in sync by hand with the enum; the
    /// test `coverage_all_clauses_seen_within_1k_iterations` will fail if
    /// you forget to update.
    pub const ALL: &'static [Clause] = &[
        Clause::Match,
        Clause::OptionalMatch,
        Clause::With,
        Clause::Unwind,
        Clause::Create,
        Clause::Return,
        Clause::ExprBinary,
        Clause::ExprFunctionCall,
        Clause::ExprList,
        Clause::ExprMap,
        Clause::ExprPropertyAccess,
        Clause::RelPattern,
        Clause::OrderBy,
        Clause::Skip,
        Clause::Limit,
        Clause::Distinct,
    ];

    /// Human-readable name, for `Display`-style reporting.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Clause::Match => "MATCH",
            Clause::OptionalMatch => "OPTIONAL_MATCH",
            Clause::With => "WITH",
            Clause::Unwind => "UNWIND",
            Clause::Create => "CREATE",
            Clause::Return => "RETURN",
            Clause::ExprBinary => "expr:binary",
            Clause::ExprFunctionCall => "expr:function_call",
            Clause::ExprList => "expr:list_literal",
            Clause::ExprMap => "expr:map_literal",
            Clause::ExprPropertyAccess => "expr:property_access",
            Clause::RelPattern => "pattern:relationship",
            Clause::OrderBy => "RETURN/ORDER_BY",
            Clause::Skip => "RETURN/SKIP",
            Clause::Limit => "RETURN/LIMIT",
            Clause::Distinct => "RETURN/DISTINCT",
        }
    }
}

/// Process-wide coverage counter — one `AtomicU64` per [`Clause`] variant.
///
/// `fuzz_structured_parse` prints a snapshot at shutdown so the nightly
/// run reports how often each clause fired. Tests also assert every
/// variant ticks at least once within 1k iterations (spec §17.4).
pub struct CoverageCounters {
    counts: [AtomicU64; Clause::ALL.len()],
}

impl CoverageCounters {
    const fn new() -> Self {
        // AtomicU64 has no `const` array-repeat constructor on stable, so
        // spell out all 16 slots explicitly. Adding a variant? Append a
        // new `AtomicU64::new(0)` below.
        Self {
            counts: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }

    /// Record one hit of `clause`.
    pub fn record(&self, clause: Clause) {
        self.counts[clause.as_index()].fetch_add(1, Ordering::Relaxed);
    }

    /// Read the current count for `clause`.
    #[must_use]
    pub fn get(&self, clause: Clause) -> u64 {
        self.counts[clause.as_index()].load(Ordering::Relaxed)
    }

    /// Reset every counter to zero. Tests that want to measure a fresh
    /// 1k-iteration window call this first.
    pub fn reset(&self) {
        for c in &self.counts {
            c.store(0, Ordering::Relaxed);
        }
    }

    /// Return every `(clause, count)` pair sorted by variant order.
    #[must_use]
    pub fn snapshot(&self) -> Vec<(Clause, u64)> {
        Clause::ALL.iter().map(|c| (*c, self.get(*c))).collect()
    }

    /// Return `true` iff every clause variant has a non-zero count.
    #[must_use]
    pub fn all_nonzero(&self) -> bool {
        Clause::ALL.iter().all(|c| self.get(*c) > 0)
    }

    /// Render a human-readable summary block suitable for printing at
    /// the end of a fuzz run. The output is stable across runs when the
    /// counts are stable (fixed iteration order).
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("[cyrs_fuzz coverage] clause hit counts:\n");
        for (clause, count) in self.snapshot() {
            out.push_str(&format!("  {:24} {}\n", clause.name(), count));
        }
        out
    }
}

/// Single process-wide instance. Safe to clone-style-read from multiple
/// threads: every field is `AtomicU64`.
pub static COVERAGE: CoverageCounters = CoverageCounters::new();

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

/// Identifier pool — intentionally short so generated statements land in
/// the Birthday-paradox regime of variable reuse, which in turn exercises
/// the scope/name-resolution machinery in `cypher-hir` and `cypher-sema`.
const VARS: &[&str] = &["n", "m", "r", "p", "x", "y", "a", "b"];

/// Node-label pool.
const LABELS: &[&str] = &["Person", "Company", "Account", "Item"];

/// Relationship-type pool.
const REL_TYPES: &[&str] = &["KNOWS", "WORKS_AT", "OWNS", "HAS"];

/// Property-key pool.
const PROP_KEYS: &[&str] = &["name", "age", "id", "score", "kind"];

/// Whitelisted function names — every entry must parse as a `FUNCTION_CALL`
/// node. We do NOT require semantic validity; `cypher-sema` may flag an
/// unknown function, but the parser accepts any IDENT followed by `(`.
const FUNCS: &[&str] = &["count", "size", "coalesce", "toLower", "toUpper", "length"];

struct Gen<'r, R: RngCore + ?Sized> {
    rng: &'r mut R,
    out: String,
    max_depth: u8,
}

impl<'r, R: RngCore + ?Sized> Gen<'r, R> {
    fn new(rng: &'r mut R, max_depth: u8) -> Self {
        Self {
            rng,
            out: String::with_capacity(256),
            max_depth,
        }
    }

    fn finish(self) -> String {
        self.out
    }

    // --- RNG helpers (deterministic, version-stable) ---------------------

    fn pick_u32(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        self.rng.next_u32() % n
    }

    /// Pick an element from a `&'static [&'static str]` pool and return it.
    fn pick_str(&mut self, slice: &[&'static str]) -> &'static str {
        let idx = self.pick_u32(slice.len() as u32) as usize;
        slice[idx]
    }

    /// Pick an element from `slice` and append it to `self.out` in one
    /// call. The inline form `self.out.push_str(self.pick_str(...))` is
    /// rejected by the borrow checker because both calls take `&mut self`;
    /// this helper hides the temporary so call sites stay readable.
    fn push_pick(&mut self, slice: &[&'static str]) {
        let s = self.pick_str(slice);
        self.out.push_str(s);
    }

    fn coin(&mut self, p_numer: u32, p_denom: u32) -> bool {
        self.pick_u32(p_denom) < p_numer
    }

    // --- statement / clauses ---------------------------------------------

    fn statement(&mut self) {
        // Every statement is a sequence of 1..=3 reading/updating clauses
        // capped with a trailing RETURN — the parser requires statements to
        // end in a returning clause for the shapes we cover here.
        //
        // We pick over five *distinct* leads (not six) so every clause has
        // the same 1/5 hit rate. Previously `_ => match_clause` biased
        // MATCH to 2/6; that meant 1k iterations still left tail clauses
        // under-covered in rare seed strides.
        let lead = self.pick_u32(5);
        match lead {
            0 => self.match_clause(),
            1 => self.optional_match_clause(),
            2 => self.with_clause(),
            3 => self.unwind_clause(),
            _ => self.create_clause(),
        }
        self.out.push(' ');
        self.return_clause();
    }

    fn match_clause(&mut self) {
        COVERAGE.record(Clause::Match);
        self.out.push_str("MATCH ");
        self.pattern();
        if self.coin(1, 2) {
            self.out.push_str(" WHERE ");
            self.expr(self.max_depth);
        }
    }

    fn optional_match_clause(&mut self) {
        COVERAGE.record(Clause::OptionalMatch);
        self.out.push_str("OPTIONAL MATCH ");
        self.pattern();
    }

    fn with_clause(&mut self) {
        COVERAGE.record(Clause::With);
        // `WITH x AS x` is the minimal shape that parses cleanly and keeps
        // the trailing RETURN well-scoped.
        self.out.push_str("WITH ");
        let v = self.pick_str(VARS);
        self.out.push_str(v);
        self.out.push_str(" AS ");
        self.out.push_str(v);
        if self.coin(1, 3) {
            self.out.push_str(" WHERE ");
            self.expr(self.max_depth);
        }
    }

    fn unwind_clause(&mut self) {
        COVERAGE.record(Clause::Unwind);
        self.out.push_str("UNWIND ");
        // Use a list literal so the parser sees a typed expression; a bare
        // variable might unresolve in sema but still parses.
        self.list_literal(self.max_depth);
        self.out.push_str(" AS ");
        self.push_pick(VARS);
    }

    fn create_clause(&mut self) {
        COVERAGE.record(Clause::Create);
        self.out.push_str("CREATE ");
        // Keep v0 simple: a single node pattern, no relationship.
        self.node_pattern();
    }

    fn return_clause(&mut self) {
        COVERAGE.record(Clause::Return);
        self.out.push_str("RETURN ");
        if self.coin(1, 4) {
            COVERAGE.record(Clause::Distinct);
            self.out.push_str("DISTINCT ");
        }
        // 1..=3 return items.
        let n = 1 + self.pick_u32(3);
        for i in 0..n {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.return_item();
        }
        if self.coin(1, 3) {
            COVERAGE.record(Clause::OrderBy);
            self.out.push_str(" ORDER BY ");
            self.push_pick(VARS);
            if self.coin(1, 2) {
                let tag = if self.coin(1, 2) { " ASC" } else { " DESC" };
                self.out.push_str(tag);
            }
        }
        if self.coin(1, 4) {
            COVERAGE.record(Clause::Skip);
            self.out.push_str(" SKIP ");
            self.int_literal();
        }
        if self.coin(1, 4) {
            COVERAGE.record(Clause::Limit);
            self.out.push_str(" LIMIT ");
            self.int_literal();
        }
    }

    fn return_item(&mut self) {
        // Either a plain variable, a property access, or an expression
        // with optional alias.
        match self.pick_u32(4) {
            0 => self.push_pick(VARS),
            1 => {
                COVERAGE.record(Clause::ExprPropertyAccess);
                self.push_pick(VARS);
                self.out.push('.');
                self.push_pick(PROP_KEYS);
            }
            _ => {
                self.expr(self.max_depth);
                if self.coin(1, 2) {
                    self.out.push_str(" AS ");
                    self.push_pick(VARS);
                }
            }
        }
    }

    // --- patterns ---------------------------------------------------------

    fn pattern(&mut self) {
        self.node_pattern();
        // Optionally extend with a relationship + node.
        if self.coin(1, 2) && self.max_depth > 0 {
            self.rel_pattern();
            self.node_pattern();
        }
    }

    fn node_pattern(&mut self) {
        self.out.push('(');
        // Variable — always emit one from the pool for reproducibility.
        self.push_pick(VARS);
        // Optional label.
        if self.coin(1, 2) {
            self.out.push(':');
            self.push_pick(LABELS);
        }
        // Optional property map.
        if self.coin(1, 3) {
            self.out.push(' ');
            self.property_map();
        }
        self.out.push(')');
    }

    fn rel_pattern(&mut self) {
        COVERAGE.record(Clause::RelPattern);
        // Random direction.
        let dir = self.pick_u32(3);
        match dir {
            1 => self.out.push_str("<-"),
            _ => self.out.push('-'),
        }
        // Optional bracketed detail.
        if self.coin(2, 3) {
            self.out.push('[');
            // Variable sometimes.
            if self.coin(1, 2) {
                self.push_pick(VARS);
            }
            if self.coin(2, 3) {
                self.out.push(':');
                self.push_pick(REL_TYPES);
            }
            if self.coin(1, 4) {
                self.out.push(' ');
                self.property_map();
            }
            self.out.push(']');
        }
        match dir {
            1 => self.out.push('-'),
            _ => self.out.push_str("->"),
        }
    }

    fn property_map(&mut self) {
        self.out.push('{');
        let n = 1 + self.pick_u32(2);
        for i in 0..n {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.push_pick(PROP_KEYS);
            self.out.push_str(": ");
            self.leaf_expr();
        }
        self.out.push('}');
    }

    // --- expressions ------------------------------------------------------

    fn expr(&mut self, depth: u8) {
        if depth == 0 {
            return self.leaf_expr();
        }
        // Branch weights: 3 leaves, 2 binops, 1 each of func/list/map.
        match self.pick_u32(8) {
            0..=2 => self.leaf_expr(),
            3..=4 => self.binary_expr(depth),
            5 => self.function_call(depth),
            6 => self.list_literal(depth),
            _ => self.map_literal(depth),
        }
    }

    fn leaf_expr(&mut self) {
        match self.pick_u32(8) {
            0 => self.int_literal(),
            1 => self.float_literal(),
            2 => self.string_literal(),
            3 => self.out.push_str("null"),
            4 => self.out.push_str("true"),
            5 => self.out.push_str("false"),
            6 => self.push_pick(VARS),
            _ => {
                // Property access.
                COVERAGE.record(Clause::ExprPropertyAccess);
                self.push_pick(VARS);
                self.out.push('.');
                self.push_pick(PROP_KEYS);
            }
        }
    }

    fn binary_expr(&mut self, depth: u8) {
        COVERAGE.record(Clause::ExprBinary);
        self.expr(depth - 1);
        // Whitelisted binary operators known to round-trip through the
        // parser. Keyword ops (AND/OR) and comparison `=` need spaces on
        // both sides; arithmetic can technically be tight but we keep it
        // spaced for the formatter's sanity.
        let op = self.pick_u32(7);
        let op_str = match op {
            0 => " + ",
            1 => " - ",
            2 => " * ",
            3 => " / ",
            4 => " AND ",
            5 => " OR ",
            _ => " = ",
        };
        self.out.push_str(op_str);
        self.expr(depth - 1);
    }

    fn function_call(&mut self, depth: u8) {
        COVERAGE.record(Clause::ExprFunctionCall);
        self.push_pick(FUNCS);
        self.out.push('(');
        // 0..=2 args. Zero is valid for `count(*)`, but we're not emitting
        // `*`; keep it to 1..=2 to simplify.
        let n = 1 + self.pick_u32(2);
        for i in 0..n {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.expr(depth - 1);
        }
        self.out.push(')');
    }

    fn list_literal(&mut self, depth: u8) {
        COVERAGE.record(Clause::ExprList);
        self.out.push('[');
        let n = self.pick_u32(4);
        for i in 0..n {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.expr(depth.saturating_sub(1));
        }
        self.out.push(']');
    }

    fn map_literal(&mut self, depth: u8) {
        COVERAGE.record(Clause::ExprMap);
        self.out.push('{');
        let n = self.pick_u32(3);
        for i in 0..n {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.push_pick(PROP_KEYS);
            self.out.push_str(": ");
            self.expr(depth.saturating_sub(1));
        }
        self.out.push('}');
    }

    // --- literals ---------------------------------------------------------

    fn int_literal(&mut self) {
        // Keep values small-ish; the lexer accepts arbitrary digits but
        // generating 20-digit integers adds no coverage.
        let v = self.pick_u32(1000);
        // Rare: negative via unary minus. Handled here as a prefix to keep
        // the leaf-caller pattern simple.
        if self.coin(1, 8) {
            self.out.push('-');
        }
        // Manual itoa to avoid pulling in std::fmt allocation noise.
        let mut buf = [0u8; 10];
        let mut i = buf.len();
        let mut v = v;
        if v == 0 {
            i -= 1;
            buf[i] = b'0';
        } else {
            while v > 0 {
                i -= 1;
                buf[i] = b'0' + (v % 10) as u8;
                v /= 10;
            }
        }
        // SAFETY-free: all bytes are ASCII digits.
        self.out.push_str(core::str::from_utf8(&buf[i..]).unwrap());
    }

    fn float_literal(&mut self) {
        // Canonical shapes: `N.M`, `N.M e±K`. We emit a fixed set of
        // representative literals rather than formatting an f64 to avoid
        // locale / precision non-determinism.
        const CHOICES: &[&str] = &["0.0", "1.5", "3.14", "2.718", "1.0e10", "1.5e-3", "0.5"];
        let pick = self.pick_str(CHOICES);
        self.out.push_str(pick);
    }

    fn string_literal(&mut self) {
        // Choose between single- and double-quoted; body is from a fixed
        // alphabet so we're not fighting UTF-8 validity concerns.
        const BODIES: &[&str] = &["alpha", "beta", "hello world", "it", ""];
        let body = self.pick_str(BODIES);
        if self.coin(1, 2) {
            self.out.push('\'');
            self.out.push_str(body);
            self.out.push('\'');
        } else {
            self.out.push('"');
            self.out.push_str(body);
            self.out.push('"');
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Unit tests live in this module so `cargo test -p cyrs-fuzz --lib` (or
// `cargo test --lib` run from fuzz/) exercises them. Note: cyrs-fuzz is
// workspace-excluded; to run tests you must `cd fuzz && cargo test` with
// a stable toolchain (libfuzzer-sys is not needed by the lib target).

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;
    use rowan::WalkEvent;

    /// Walk a parsed tree and return `true` iff it contains any ERROR
    /// node or token.
    fn has_error_nodes(s: &str) -> bool {
        let parse = cypher_syntax::parse(s);
        if !parse.errors().is_empty() {
            return true;
        }
        let root = parse.syntax();
        root.preorder_with_tokens().any(|ev| match ev {
            WalkEvent::Enter(rowan::NodeOrToken::Node(n)) => {
                n.kind() == cypher_syntax::SyntaxKind::ERROR
            }
            WalkEvent::Enter(rowan::NodeOrToken::Token(t)) => {
                t.kind() == cypher_syntax::SyntaxKind::ERROR
            }
            _ => false,
        })
    }

    /// Property: for every seed in `0..iters`, the generator emits Cypher
    /// that parses with zero ERROR nodes and an empty `errors()` list.
    #[test]
    fn generator_emits_clean_parses() {
        const ITERS: u64 = 1000;
        let mut failures: Vec<(u64, String)> = Vec::new();
        for seed in 0..ITERS {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            // Vary max_depth across seeds to hit both shallow and deep
            // branches.
            let depth = (seed % 5) as u8 + 1;
            let src = random_valid_cypher(&mut rng, depth);
            if has_error_nodes(&src) {
                failures.push((seed, src));
                if failures.len() > 5 {
                    break;
                }
            }
        }
        assert!(
            failures.is_empty(),
            "generator produced sources with ERROR nodes:\n{}",
            failures
                .iter()
                .map(|(seed, s)| format!("  seed={seed}: {s:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Property: same seed + same depth ⇒ byte-identical output.
    #[test]
    fn generator_is_deterministic() {
        for seed in [0u64, 1, 42, 0xdead_beef_cafe_babe] {
            let mut a = ChaCha8Rng::seed_from_u64(seed);
            let mut b = ChaCha8Rng::seed_from_u64(seed);
            let sa = random_valid_cypher(&mut a, 4);
            let sb = random_valid_cypher(&mut b, 4);
            assert_eq!(sa, sb, "non-deterministic at seed={seed}");
        }
    }

    /// Property: `max_depth=0` still produces a syntactically valid
    /// statement (all recursive branches short-circuit to leaves).
    #[test]
    fn generator_respects_depth_zero() {
        for seed in 0..64u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let src = random_valid_cypher(&mut rng, 0);
            assert!(
                !has_error_nodes(&src),
                "ERROR at depth=0 seed={seed}: {src:?}"
            );
        }
    }

    /// Clause-coverage acceptance test (bead cy-h07, spec §17.4).
    ///
    /// Every [`Clause`] variant must tick at least once across 1000
    /// iterations with a fixed seed stride. This guards against future
    /// edits to the generator (e.g. re-weighting the clause picker)
    /// silently dropping a branch from the fuzz distribution.
    ///
    /// We reset `COVERAGE` at the start so previous tests in the same
    /// process don't mask a regression. Rust's default test runner
    /// parallelises test threads, so we use a local `CoverageCounters`
    /// instance populated by a per-test hook — but since the generator
    /// writes to the static `COVERAGE`, we instead take a "before"
    /// snapshot and assert each clause's *delta* is > 0.
    #[test]
    fn coverage_all_clauses_seen_within_1k_iterations() {
        const ITERS: u64 = 1000;

        // Snapshot before running so parallel tests in this process
        // (which also call `random_valid_cypher`) don't interfere.
        let before: Vec<u64> = Clause::ALL.iter().map(|c| COVERAGE.get(*c)).collect();

        for seed in 0..ITERS {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            // Vary depth so shallow AND deep branches (map, list, binary,
            // function call) all get a realistic shot at firing.
            let depth = (seed % 5) as u8 + 2;
            let _ = random_valid_cypher(&mut rng, depth);
        }

        let mut missing: Vec<&'static str> = Vec::new();
        for (i, clause) in Clause::ALL.iter().enumerate() {
            let delta = COVERAGE.get(*clause) - before[i];
            if delta == 0 {
                missing.push(clause.name());
            }
        }

        assert!(
            missing.is_empty(),
            "clause coverage gap after {ITERS} iterations: {missing:?}"
        );

        // Emit the per-clause hit counts so `cargo test -- --nocapture`
        // doubles as the coverage-metric output promised by cy-h07.
        eprintln!("[cy-h07] clause coverage over {ITERS} iterations:");
        for (i, clause) in Clause::ALL.iter().enumerate() {
            let delta = COVERAGE.get(*clause) - before[i];
            eprintln!("  {:24} {}", clause.name(), delta);
        }
    }

    /// `COVERAGE.summary()` lists every variant and is reasonably sized.
    /// Guards the diagnostic-quality requirement that the fuzz runner can
    /// print a coverage summary at shutdown (cy-h07 scope bullet 2).
    #[test]
    fn coverage_summary_mentions_every_variant() {
        let summary = COVERAGE.summary();
        for clause in Clause::ALL {
            assert!(
                summary.contains(clause.name()),
                "summary missing variant {:?}:\n{summary}",
                clause.name()
            );
        }
    }
}
