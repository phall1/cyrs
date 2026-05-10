//! cy-01q — Map projection `n { .p, key: v, .*, * }` parser tests.
//!
//! Spec §7.3 / §19. The trailing `{ ... }` is recognised as a postfix
//! trailer on an atom expression; standalone `{ k: v }` map literals
//! still parse via the atom path.

use cypher_syntax::parse;

#[test]
fn map_projection_property_selectors() {
    let p = parse("RETURN n { .name, .age }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let tree = format!("{:#?}", p.syntax());
    assert!(
        tree.contains("MAP_PROJECTION"),
        "no MAP_PROJECTION node: {tree}"
    );
    assert!(
        tree.contains("MAP_PROJECTION_ITEM"),
        "no MAP_PROJECTION_ITEM: {tree}"
    );
}

#[test]
fn map_projection_literal_items() {
    let p = parse("RETURN n { name: n.name, age: 42 }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn map_projection_mixed_items() {
    let p = parse("RETURN n { .name, .age, full: n.first + ' ' + n.last, * }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn map_projection_all_properties_spread() {
    let p = parse("RETURN n { .* }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn map_projection_all_bindings_spread() {
    let p = parse("RETURN n { * }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn map_projection_after_property_access() {
    // Subject can be any atom expression, including a property access.
    let p = parse("RETURN user.profile { .bio, .avatar }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn standalone_map_literal_still_parses() {
    // `RETURN { k: v }` — the `{` is in atom position, not a trailer,
    // so this must still parse as a MAP_LITERAL, not a MAP_PROJECTION.
    let p = parse("RETURN { k: 1, k2: 2 }");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let tree = format!("{:#?}", p.syntax());
    assert!(tree.contains("MAP_LITERAL"), "no MAP_LITERAL: {tree}");
    assert!(
        !tree.contains("MAP_PROJECTION"),
        "spurious MAP_PROJECTION: {tree}"
    );
}

#[test]
fn map_projection_unclosed_recovers() {
    let p = parse("RETURN n { .name");
    assert!(!p.errors().is_empty(), "expected at least one error");
    let codes: Vec<u16> = p.errors().iter().map(|e| e.code).collect();
    assert!(
        codes.contains(&78),
        "expected EXPECTED_RBRACE_MAP_PROJ (78), got {codes:?}"
    );
}
