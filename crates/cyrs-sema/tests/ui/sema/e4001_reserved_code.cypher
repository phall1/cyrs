// E4001: reserved sentinel code (not emitted directly by any pass).
//
// E4001 is a sentinel range-boundary code, not emitted by any current pass.
// Dialect gate codes begin at E4010. This file serves as a placeholder for
// the E4xxx range; the actual gates (E4010-E4019) are tested in insta unit
// tests in dialect.rs and via e4016_*.cypher once CALL lowering lands.
MATCH (n) RETURN n
