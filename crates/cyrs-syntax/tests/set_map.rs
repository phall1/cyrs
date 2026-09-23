//! `SET n = map` and `SET n += map` parse as set items. Spec 0005 §3.

use cyrs_syntax::parse;

#[test]
fn set_equals_map_parses() {
    let p = parse("MATCH (n) SET n = {a: 1}");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn set_plus_equals_map_parses() {
    let p = parse("MATCH (n) SET n += {a: 1}");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let tree = format!("{:#?}", p.syntax());
    assert!(
        tree.contains("PLUS"),
        "+= should keep the PLUS token: {tree}"
    );
}
