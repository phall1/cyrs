// Joins both Person and Movie plus the ACTED_IN relationship type — all
// three declared in ../schema.toml. Demonstrates a single query can mix
// labels / rel types contributed by one shared schema file across the
// whole project.
MATCH (p:Person)-[r:ACTED_IN]->(m:Movie)
RETURN p.name, m.title
