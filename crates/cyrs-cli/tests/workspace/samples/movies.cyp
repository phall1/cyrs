// `Movie` is declared in ../schema.toml and reachable from a different
// file than `Person` below — the workspace `Database` merges both
// declarations into a single SchemaProvider.
MATCH (m:Movie) RETURN m.title
