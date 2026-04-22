//! bench_plan — HIR → plan lowering (spec §17.10).
//!
//! Measures `cypher_plan::lower::lower_statement` on a representative query.
//! The HIR is built once outside the loop so that only plan lowering is measured.

use criterion::{Criterion, criterion_group, criterion_main};

const QUERY: &str = r#"
MATCH (n:Person {name: $name})-[r:KNOWS*1..3]->(m:Person)
WHERE m.age > 30 AND m.active = true
WITH n, m, r, count(*) AS hops
ORDER BY hops ASC
LIMIT 100
RETURN n.name AS source, m.name AS target, hops
"#;

fn bench_plan(c: &mut Criterion) {
    let stmt = cypher_hir::lower::lower_statement(QUERY);

    c.bench_function("plan", |b| {
        b.iter(|| {
            // cy-wlr: lowering is fallible; swallow the Result shape inside
            // the hot loop so we're measuring the same work as before.
            let _plan = cypher_plan::lower::lower_statement(std::hint::black_box(&stmt));
        });
    });
}

criterion_group!(benches, bench_plan);
criterion_main!(benches);
