//! bench_sema — semantic analysis (spec §17.10).
//!
//! Measures `cyrs_sema::analyse` on a representative multi-clause query.
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
    let stmt = cyrs_hir::lower::lower_statement(QUERY).expect("bench query must lower cleanly");
    let opts = cyrs_sema::SemaOptions::default();

    c.bench_function("sema", |b| {
        b.iter(|| {
            let mut sink = cyrs_diag::DiagnosticsSink::new();
            cyrs_sema::analyse(
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
