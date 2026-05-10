// `Person` is declared in ../schema.toml; the workspace loader makes it
// visible to this member file.
MATCH (p:Person) RETURN p.name
