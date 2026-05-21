//! L3 — missing label / relationship-type in a pattern
//! ([`W6013`](cyrs_diag::DiagCode::W6013)).
//!
//! Flags a `MATCH` node pattern with no label, or a relationship
//! pattern with no type. Such a pattern matches *every* node /
//! relationship in the graph — almost always an unintended full scan
//! and, when the author simply forgot the label, a latent bug.
//!
//! # Schema-aware
//!
//! This lint runs only when a [`SchemaProvider`] is supplied (the
//! dispatcher in [`super`] gates it). Without a schema there is no
//! catalogue of labels to suggest and an unlabelled pattern may be
//! perfectly intentional; with one, an unrestricted pattern stands out
//! against the declared shape.
//!
//! To stay quiet on genuinely schema-less graphs, the lint additionally
//! requires the schema to *declare at least one* label (for node
//! patterns) or relationship type (for relationship patterns). An empty
//! schema yields no diagnostics.
//!
//! # Scope
//!
//! Only `MATCH` / `OPTIONAL MATCH` patterns are linted. A `CREATE`
//! without a label is a different construct (it creates an unlabelled
//! node on purpose) and is out of scope.

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::{Clause, PatternElement, Statement, VarId};
use cyrs_schema::SchemaProvider;

/// Run L3 over `stmt` against `schema`.
pub fn check(stmt: &Statement, schema: &dyn SchemaProvider, sink: &mut DiagnosticsSink) {
    let has_labels = !schema.labels().is_empty();
    let has_rel_types = !schema.relationship_types().is_empty();
    if !has_labels && !has_rel_types {
        // Schema declares no shape at all — nothing actionable.
        return;
    }

    for clause in &stmt.clauses {
        let Clause::Match { pattern, .. } = clause else {
            continue;
        };
        for part in &pattern.parts {
            for elem in &part.elements {
                match elem {
                    PatternElement::Node {
                        bind, labels, span, ..
                    } if has_labels && labels.is_empty() => {
                        emit_node(stmt, *bind, *span, sink);
                    }
                    PatternElement::Rel {
                        bind, types, span, ..
                    } if has_rel_types && types.is_empty() => {
                        emit_rel(stmt, *bind, *span, sink);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn emit_node(
    stmt: &Statement,
    bind: Option<VarId>,
    span: cyrs_hir::HirSpan,
    sink: &mut DiagnosticsSink,
) {
    let who = describe(stmt, bind, "node");
    sink.push(
        Diagnostic::warning(
            DiagCode::W6013,
            span,
            format!("{who} has no label — the pattern will scan every node"),
        )
        .with_note(
            "add a label (e.g. `(n:Label)`) to restrict the match to the \
             intended node kind",
        ),
    );
}

fn emit_rel(
    stmt: &Statement,
    bind: Option<VarId>,
    span: cyrs_hir::HirSpan,
    sink: &mut DiagnosticsSink,
) {
    let who = describe(stmt, bind, "relationship");
    sink.push(
        Diagnostic::warning(
            DiagCode::W6013,
            span,
            format!("{who} has no relationship type — the pattern will scan every relationship"),
        )
        .with_note(
            "add a relationship type (e.g. `-[r:TYPE]->`) to restrict the \
             match to the intended relationship kind",
        ),
    );
}

fn describe(stmt: &Statement, bind: Option<VarId>, kind: &str) -> String {
    match bind.and_then(|v| stmt.bindings.get(&v)) {
        Some(b) => format!("{kind} pattern `{}`", b.name),
        None => format!("anonymous {kind} pattern"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::test_support::render;
    use cyrs_diag::DiagnosticsSink;
    use cyrs_hir::lower_statement;
    use cyrs_schema::{EndpointDecl, FunctionSignature, ProcedureSignature, PropertyDecl};
    use smol_str::SmolStr;

    /// Minimal schema: one label, one relationship type.
    #[derive(Debug)]
    struct TestSchema;

    impl SchemaProvider for TestSchema {
        fn labels(&self) -> Vec<SmolStr> {
            vec![SmolStr::new("Person")]
        }
        fn relationship_types(&self) -> Vec<SmolStr> {
            vec![SmolStr::new("KNOWS")]
        }
        fn node_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
            None
        }
        fn relationship_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
            None
        }
        fn relationship_endpoints(&self, _: &str) -> Vec<EndpointDecl> {
            Vec::new()
        }
        fn inverse_of(&self, _: &str) -> Option<SmolStr> {
            None
        }
        fn function(&self, _: &str) -> Option<FunctionSignature> {
            None
        }
        fn procedure(&self, _: &str) -> Option<ProcedureSignature> {
            None
        }
        fn schema_digest(&self) -> [u8; 32] {
            [0; 32]
        }
    }

    fn run(src: &str) -> String {
        let stmt = lower_statement(src).expect("test input lowers");
        let mut sink = DiagnosticsSink::new();
        check(&stmt, &TestSchema, &mut sink);
        render(src, sink)
    }

    #[test]
    fn snap_unlabelled_node_fires() {
        insta::assert_snapshot!(run("MATCH (n) RETURN n"));
    }

    #[test]
    fn snap_labelled_node_clean() {
        insta::assert_snapshot!(run("MATCH (n:Person) RETURN n"));
    }

    #[test]
    fn untyped_relationship_fires() {
        let out = run("MATCH (n:Person)-[r]->(m:Person) RETURN r");
        assert!(out.contains("W6013"), "{out}");
        assert!(out.contains("no relationship type"), "{out}");
    }

    #[test]
    fn typed_relationship_clean() {
        let out = run("MATCH (n:Person)-[r:KNOWS]->(m:Person) RETURN r");
        assert!(out.contains("diagnostics: 0"), "{out}");
    }
}
