//! bench_format — CST-driven formatter (spec §17.10).
//!
//! Measures `cypher_fmt::format` on a representative multi-clause query.

use criterion::{Criterion, criterion_group, criterion_main};

const QUERY: &str = r#"
MATCH (n:Person {name: $name})-[r:KNOWS*1..3]->(m:Person)
WHERE m.age > 30 AND m.active = true
WITH n, m, r, count(*) AS hops
ORDER BY hops ASC
LIMIT 100
RETURN n.name AS source, m.name AS target, hops
"#;

fn bench_format(c: &mut Criterion) {
    c.bench_function("format", |b| {
        b.iter(|| {
            let _out = cypher_fmt::format(std::hint::black_box(QUERY));
        });
    });
}

criterion_group!(benches, bench_format);
criterion_main!(benches);
