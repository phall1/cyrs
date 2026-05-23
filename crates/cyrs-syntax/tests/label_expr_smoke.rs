//! cy-p3cl: smoke tests for the §16.4 label-expression operators.

use cyrs_syntax::parse;

fn err_count(src: &str) -> usize {
    parse(src).errors().len()
}

fn round_trip_ok(src: &str) -> bool {
    parse(src).syntax().to_string() == src
}

#[test]
fn accepts_conjunction() {
    let src = "MATCH (n:A&B) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn accepts_disjunction() {
    let src = "MATCH (n:A|B) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn accepts_negation() {
    let src = "MATCH (n:!A) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn accepts_wildcard() {
    let src = "MATCH (n:%) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn accepts_paren_compound() {
    let src = "MATCH (n:(A|B)&C) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn legacy_single_label_still_ok() {
    let src = "MATCH (n:A) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn legacy_colon_conjunction_still_ok() {
    let src = "MATCH (n:A:B) RETURN n";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn rel_type_single_unaffected() {
    // The rel-type parser admits only `:Type` today; the `|` rel-type
    // disjunction sits on a separate worklist bead and is intentionally
    // NOT pulled in by cy-p3cl's label-expression work. We exercise
    // the single-rel-type happy path to prove rel-type parsing was not
    // disturbed by the new node-pattern label code.
    let src = "MATCH ()-[:KNOWS]->() RETURN 1";
    assert_eq!(err_count(src), 0, "errors: {:?}", parse(src).errors());
    assert!(round_trip_ok(src));
}

#[test]
fn bang_without_primary_errors() {
    let src = "MATCH (n:!) RETURN n";
    let p = parse(src);
    assert!(
        p.errors().iter().any(|e| format!("{e:?}").contains("101")),
        "errors: {:?}",
        p.errors()
    );
}

#[test]
fn unclosed_paren_errors() {
    // Inner label-paren never closed before the node-pattern's `)` is
    // consumed by the disjunction parser as a primary.
    let src = "MATCH (n:(A&B RETURN n";
    let p = parse(src);
    assert!(
        p.errors().iter().any(|e| format!("{e:?}").contains("102")),
        "errors: {:?}",
        p.errors()
    );
}
