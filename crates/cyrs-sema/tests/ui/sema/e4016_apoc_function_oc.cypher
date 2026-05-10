// E4016: APOC vendor-prefixed procedure denied in OpenCypherV9 dialect.
// dialect: OpenCypherV9
//
// Note: same deferred-CALL-lowering limitation as e4016_apoc_function_gql.
// E4016 in OpenCypherV9 mode is covered by insta unit tests in dialect.rs.
MATCH (n) RETURN n
