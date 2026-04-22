// `Person` is declared in ../schema.toml. Workspace mode resolves it
// across files, so this checks clean.
MATCH (p:Person) RETURN p.name
