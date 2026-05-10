//! Worked example: implementing [`SchemaProvider`] against a pre-existing
//! "catalog" type owned by an embedder.
//!
//! This is the migration template referenced from `docs/sema-checks.md`
//! and from issue cy-emb4. It shows the smallest plausible adapter
//! between an embedder-owned catalog and `cyrs-sema`'s schema-aware
//! analysis pass.
//!
//! Run it with:
//!
//! ```text
//! cargo run --example embedder_adapter -p cyrs-sema
//! ```
//!
//! The example query is:
//!
//! ```cypher
//! MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name
//! ```
//!
//! Two runs are performed:
//!
//! 1. With a [`ToyCatalog`] that declares `Person` and `KNOWS`. The
//!    pipeline produces zero schema-aware diagnostics.
//! 2. The same query against [`cyrs_schema::EmptySchema`] — every
//!    label/relationship/property reference becomes an `E3xxx`
//!    diagnostic. This is what an embedder sees today before wiring up
//!    a real catalog adapter.
//!
//! The split makes it obvious which checks `cyrs-sema` runs (schema-
//! aware) and which it does not (storage validation, runtime types,
//! authorisation — all left to the embedder; see `docs/sema-checks.md`).

use cyrs_diag::{DiagnosticsSink, render_text_string};
use cyrs_hir::{desugar::desugar_statement, lower::lower_statement, scope::Resolution};
use cyrs_schema::{
    Cardinality, EmptySchema, EndpointDecl, FunctionSignature, ProcedureSignature, PropertyDecl,
    PropertyType, SchemaProvider,
};
use cyrs_sema::{SemaOptions, analyse, resolve};
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// 1. Toy embedder catalog
// ---------------------------------------------------------------------------

/// A tiny schema. In a real embedder this would be the existing
/// in-process catalog (loaded from disk, fetched from the storage
/// engine, etc.) — only the *adapter* lives in the embedder; the
/// catalog itself stays put.
#[derive(Debug, Default)]
struct ToyCatalog {
    /// Declared node kinds: `(label, properties)`.
    node_kinds: Vec<(SmolStr, Vec<PropertyDecl>)>,
    /// Declared relationship kinds:
    /// `(rel_type, properties, allowed endpoints)`.
    rel_kinds: Vec<(SmolStr, Vec<PropertyDecl>, Vec<EndpointDecl>)>,
}

impl ToyCatalog {
    /// Build a catalog with `Person { name: String }` and
    /// `KNOWS: Person -> Person`.
    fn person_knows() -> Self {
        let mut c = ToyCatalog::default();
        c.add_node(
            "Person",
            vec![PropertyDecl::new("name", PropertyType::String, true)],
        );
        c.add_rel(
            "KNOWS",
            vec![],
            vec![EndpointDecl {
                from: SmolStr::new("Person"),
                to: SmolStr::new("Person"),
                cardinality: Cardinality::ManyToMany,
            }],
        );
        c
    }

    fn add_node(&mut self, label: &str, props: Vec<PropertyDecl>) {
        self.node_kinds.push((SmolStr::new(label), props));
    }

    fn add_rel(&mut self, rel_type: &str, props: Vec<PropertyDecl>, endpoints: Vec<EndpointDecl>) {
        self.rel_kinds
            .push((SmolStr::new(rel_type), props, endpoints));
    }
}

// ---------------------------------------------------------------------------
// 2. The adapter — `impl SchemaProvider for ToyCatalog`
// ---------------------------------------------------------------------------
//
// Each method's job is documented below in line; the pattern here is
// representative of what a real embedder needs to write, regardless of
// whether the underlying catalog is a Vec, a HashMap, or a database
// handle.

impl SchemaProvider for ToyCatalog {
    fn labels(&self) -> Vec<SmolStr> {
        self.node_kinds.iter().map(|(l, _)| l.clone()).collect()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.rel_kinds.iter().map(|(t, _, _)| t.clone()).collect()
    }

    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        // Returning `None` for an unknown label triggers `E3001`
        // *only* at use-sites — `node_properties` is only consulted
        // after the label is known, so callers usually pre-check with
        // `has_label`.
        self.node_kinds
            .iter()
            .find(|(l, _)| l.as_str() == label)
            .map(|(_, props)| props.clone())
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>> {
        self.rel_kinds
            .iter()
            .find(|(t, _, _)| t.as_str() == rel_type)
            .map(|(_, props, _)| props.clone())
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
        // Empty vec = endpoint-polymorphic; the pass then skips
        // endpoint validation. If the embedder's catalog has
        // open-ended rel types, returning empty here is correct.
        self.rel_kinds
            .iter()
            .find(|(t, _, _)| t.as_str() == rel_type)
            .map(|(_, _, eps)| eps.clone())
            .unwrap_or_default()
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<SmolStr> {
        // No typed inverses in this toy catalog.
        None
    }

    fn function(&self, _name: &str) -> Option<FunctionSignature> {
        // The embedder's catalog only describes graph structure; it
        // delegates the function catalog to `StandardLibrary`.
        // (Real adapters typically return `None` here and let
        // callers wrap the schema with `StandardLibrary::wrap`.)
        None
    }

    fn procedure(&self, _name: &str) -> Option<ProcedureSignature> {
        None
    }

    fn schema_digest(&self) -> [u8; 32] {
        // Replace with a real content-addressed hash in production
        // (BLAKE3 / SHA-256 of the catalog's serialised surface).
        // The digest MUST change whenever any observable schema
        // surface changes; otherwise Salsa caches stale results.
        let mut out = [0u8; 32];
        out[0] = u8::try_from(self.node_kinds.len()).unwrap_or(u8::MAX);
        out[1] = u8::try_from(self.rel_kinds.len()).unwrap_or(u8::MAX);
        out
    }
}

// ---------------------------------------------------------------------------
// 3. Driver
// ---------------------------------------------------------------------------

const QUERY: &str = "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name";

fn run_with(label: &str, schema: Option<&dyn SchemaProvider>) {
    println!("== {label} ==");
    println!("query: {QUERY}");

    let stmt = desugar_statement(lower_statement(QUERY));

    // Resolved bindings: report how many use-sites the resolver
    // bound. `cyrs-sema::analyse` runs the same resolver pass
    // internally; we re-run it here only to surface the table.
    let mut resolve_sink = DiagnosticsSink::new();
    let resolved = resolve::resolve(&stmt, /* warn_shadowing */ false, &mut resolve_sink);

    let total = resolved.resolved_names.len();
    let unresolved = resolved.resolved_names.unresolved_count();
    println!("bindings: {total} use-sites total, {unresolved} unresolved");

    for (var_id, res) in resolved.resolved_names.iter() {
        match res {
            Resolution::Resolved(b) => println!(
                "  {var_id:?} -> defining {:?} (kind {:?})",
                b.defining_var, b.kind
            ),
            Resolution::Unresolved => println!("  {var_id:?} -> UNRESOLVED"),
        }
    }

    // Full analysis: drives every sema pass against the same
    // `SchemaProvider`. Embedders only call this entry point.
    let mut sink = DiagnosticsSink::new();
    let opts = SemaOptions::default();
    analyse(&stmt, schema, &opts, &mut sink);
    let diags = sink.into_sorted();

    if diags.is_empty() {
        println!("diagnostics: (none — schema accepted the query)");
    } else {
        println!("diagnostics: {} emitted", diags.len());
        for d in &diags {
            print!(
                "{}",
                render_text_string("embedder_adapter.cypher", QUERY, d)
            );
        }
    }
    println!();
}

fn main() {
    // Run 1 — the toy catalog declares `Person` and `KNOWS`, so the
    // schema-aware pass produces no `E3xxx` diagnostics.
    let toy = ToyCatalog::person_knows();
    run_with(
        "ToyCatalog (Person + KNOWS declared)",
        Some(&toy as &dyn SchemaProvider),
    );

    // Run 2 — the empty schema declares nothing; every label /
    // rel-type / function reference becomes an unknown.
    let empty = EmptySchema;
    run_with(
        "EmptySchema (no declarations)",
        Some(&empty as &dyn SchemaProvider),
    );

    // Run 3 — schema-free baseline. Only structural / kind / clause-
    // ordering errors surface; labels and rel-types are accepted
    // unconditionally because no catalog is consulted.
    run_with("schema-free (no SchemaProvider supplied)", None);
}
