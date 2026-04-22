//! bench_sema — semantic analysis (spec §17.10).
//!
//! Measures `cypher_sema::analyse` on a representative multi-clause query.
//! The HIR is built once outside the loop so that only analysis is measured.

use criterion::{Criterion, criterion_group, criterion_main};

const QUERY: &str = r#"
MATCH (n:Person {name: $name})-[r:KNOWS*1..3]->(m:Person)
WHERE m.age > 30 AND m.active = true
WITH n, m, r, count(*) AS hops
ORDER BY hops ASC
LIMIT 100
RETURN n.name AS source, m.name AS target, hops
"#;

fn bench_sema(c: &mut Criterion) {
    let stmt = cypher_hir::lower::lower_statement(QUERY);
    let opts = cypher_sema::SemaOptions::default();

    c.bench_function("sema", |b| {
        b.iter(|| {
            let mut sink = cypher_diag::DiagnosticsSink::new();
            cypher_sema::analyse(
                std::hint::black_box(&stmt),
                None,
                std::hint::black_box(&opts),
                &mut sink,
            );
        });
    });
}

criterion_group!(benches, bench_sema);
criterion_main!(benches);
