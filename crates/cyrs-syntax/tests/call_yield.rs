//! cy-4mg — `CALL <proc> YIELD ...` standalone form syntax tests.
//!
//! Spec §14 / §19. The block form `CALL { <subquery> }` is a separate
//! follow-up bead.

use cyrs_syntax::parse;

#[test]
fn call_yield_parses_clean() {
    let p = parse("CALL db.labels() YIELD label RETURN label");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let tree = format!("{:#?}", p.syntax());
    assert!(tree.contains("CALL_CLAUSE"), "no CALL_CLAUSE: {tree}");
    assert!(tree.contains("PROCEDURE_NAME"), "no PROCEDURE_NAME: {tree}");
    assert!(
        tree.contains("YIELD_SUBCLAUSE"),
        "no YIELD_SUBCLAUSE: {tree}"
    );
    assert!(tree.contains("YIELD_ITEM"), "no YIELD_ITEM: {tree}");
}

#[test]
fn call_yield_with_alias() {
    let p = parse("CALL db.labels() YIELD label AS l RETURN l");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn call_no_args_no_yield() {
    let p = parse("CALL db.ping");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn call_multi_arg_multi_yield() {
    let p = parse("CALL db.foo(1, 2, 'x') YIELD a, b AS bb RETURN a, bb");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn call_qualified_three_segments() {
    let p = parse("CALL ns.module.proc()");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn call_missing_proc_name_recovers() {
    // `CALL` without a name should produce a diagnostic, not a panic.
    let p = parse("CALL");
    assert!(!p.errors().is_empty(), "expected at least one error");
    let codes: Vec<u16> = p.errors().iter().map(|e| e.code).collect();
    assert!(
        codes.contains(&73),
        "expected EXPECTED_PROCEDURE_NAME (73), got {codes:?}"
    );
}
