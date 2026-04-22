// `Ghost` is not in the schema — expect an unknown-label diagnostic.
MATCH (g:Ghost) RETURN g
