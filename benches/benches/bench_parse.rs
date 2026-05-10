//! bench_parse — lexer + recovering parser (spec §17.10).
//!
//! Measures `cyrs_syntax::parse` on a representative multi-clause query.

use criterion::{Criterion, criterion_group, criterion_main};

/// A realistic multi-clause Cypher query used across all benches.
const QUERY: &str = r#"
MATCH (n:Person {name: $name})-[r:KNOWS*1..3]->(m:Person)
WHERE m.age > 30 AND m.active = true
WITH n, m, r, count(*) AS hops
ORDER BY hops ASC
LIMIT 100
RETURN n.name AS source, m.name AS target, hops
"#;

fn bench_parse(c: &mut Criterion) {
    c.bench_function("parse", |b| {
        b.iter(|| {
            let _parse = cyrs_syntax::parse(std::hint::black_box(QUERY));
        });
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
