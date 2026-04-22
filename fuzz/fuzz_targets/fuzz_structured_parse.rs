//! fuzz_structured_parse — grammar-aware fuzz oracle (bead cy-h07.1).
//!
//! Unlike the byte-level targets, this one uses libFuzzer's input as an
//! RNG seed, emits a syntactically valid Cypher statement via
//! `cyrs_fuzz::generator::random_valid_cypher`, and asserts:
//!
//! 1. The generator's output parses with **zero** ERROR nodes. If this
//!    ever fires it's a generator bug, not a parser bug — but we want to
//!    catch both classes here because the whole point of the structured
//!    target is that the parser sees valid input.
//! 2. `fmt(parse(fmt(s))) == fmt(s)` — semantic-preservation under
//!    formatting (spec §17.3 P17.3.4 / P17.3.3). The formatter is
//!    supposed to round-trip on valid input; this is the same oracle
//!    `crates/cypher-fmt/tests/properties.rs` uses, exercised over a
//!    much larger distribution of shapes than proptest's strategies cover.
//! 3. Every downstream stage (HIR lowering, sema, plan) runs without
//!    panic on the generated source. Adds breadth coverage for free.
//!
//! Any failure panics with the full generated source attached so
//! libFuzzer can minimise the RNG seed (via `cargo fuzz tmin`) and
//! surface a deterministic reproducer.
//!
//! spec §17.4.

#![no_main]

use cyrs_fuzz::generator::random_valid_cypher;
use libfuzzer_sys::fuzz_target;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

fuzz_target!(|data: &[u8]| {
    // Need at least 8 bytes to seed ChaCha8 meaningfully; smaller inputs
    // would over-fit libFuzzer's tiny initial corpus to a handful of
    // outputs and waste iterations.
    if data.len() < 8 {
        return;
    }

    let mut seed = [0u8; 32];
    // Fill the seed buffer from `data`, cycling if data is short.
    // Everything past 32 bytes is ignored; libFuzzer will still feed us
    // varied prefixes.
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = data[i % data.len()];
    }
    let mut rng = ChaCha8Rng::from_seed(seed);

    // Depth is picked from the input's first byte so it varies across
    // corpus entries without exhausting the RNG stream.
    let depth = (data[0] % 6) + 1;
    let src = random_valid_cypher(&mut rng, depth);

    // -------- Oracle 1: parser produces no ERROR nodes --------
    let parsed = cypher_syntax::parse(&src);
    if !parsed.errors().is_empty() {
        panic!(
            "generator produced input with parser errors: {src:?}\nerrors: {:?}",
            parsed.errors()
        );
    }
    let root = parsed.syntax();
    let has_error_node = root.preorder_with_tokens().any(|ev| match ev {
        rowan::WalkEvent::Enter(rowan::NodeOrToken::Node(n)) => {
            n.kind() == cypher_syntax::SyntaxKind::ERROR
        }
        rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(t)) => {
            t.kind() == cypher_syntax::SyntaxKind::ERROR
        }
        _ => false,
    });
    assert!(
        !has_error_node,
        "generator produced input that hit recovery: {src:?}"
    );

    // -------- Oracle 2: formatter idempotence + semantic preservation --------
    let once = cypher_fmt::format(&src);
    let twice = cypher_fmt::format(&once);
    assert_eq!(
        once, twice,
        "formatter not idempotent:\n  src: {src:?}\n  once: {once:?}\n  twice: {twice:?}"
    );

    // parse(fmt(s)) must also be clean on valid input. If it isn't, the
    // formatter reshaped a parseable source into an unparseable one.
    let reparsed = cypher_syntax::parse(&once);
    assert!(
        reparsed.errors().is_empty(),
        "fmt(src) no longer parses:\n  src: {src:?}\n  fmt: {once:?}\n  errors: {:?}",
        reparsed.errors()
    );
    let reparsed_root = reparsed.syntax();
    let reparsed_has_error = reparsed_root.preorder_with_tokens().any(|ev| match ev {
        rowan::WalkEvent::Enter(rowan::NodeOrToken::Node(n)) => {
            n.kind() == cypher_syntax::SyntaxKind::ERROR
        }
        rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(t)) => {
            t.kind() == cypher_syntax::SyntaxKind::ERROR
        }
        _ => false,
    });
    assert!(
        !reparsed_has_error,
        "fmt(src) reparses with ERROR nodes:\n  src: {src:?}\n  fmt: {once:?}"
    );

    // -------- Oracle 3: downstream stages run without panic --------
    // Lower to HIR and run sema; any panic is a blocker. We do not
    // assert on diagnostic content — `cypher-sema` may legitimately
    // flag the generator's output (e.g. unknown function calls).
    let stmt = cypher_hir::lower::lower_statement(&src);
    let mut sink = cypher_diag::DiagnosticsSink::new();
    let opts = cypher_sema::SemaOptions::default();
    cypher_sema::analyse(&stmt, None, &opts, &mut sink);

    // Plan lowering is intentionally NOT exercised here: per
    // `cypher_plan::lower::lower_statement` (bead cy-b4b), the HIR must
    // have been name-resolved first, and the byte-level `fuzz_plan`
    // target already covers the raw-input panic surface. Feeding our
    // generator's HIR into the plan lowerer would trip the explicit
    // "run name resolution before calling lower_statement" assertion on
    // every statement containing a variable reference — a precondition
    // violation, not a bug we should surface here.
});
