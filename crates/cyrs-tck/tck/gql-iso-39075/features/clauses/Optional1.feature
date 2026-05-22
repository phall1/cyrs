#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §14.7 (optional match statement) and §14.11
# (call statement).  `OPTIONAL CALL` is GQL-distinct: the `OPTIONAL`
# qualifier on `CALL` returns the empty multiset on procedure failure,
# rather than propagating an error.
#

#encoding: utf-8

Feature: Optional1 - OPTIONAL qualifiers (GQL-distinct)

  # ISO/IEC 39075:2024 §14.11.3 — `OPTIONAL CALL` qualifier.
  @covers:callProcedureStatement,callQueryStatement,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] OPTIONAL CALL of side-effect-free procedure
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      OPTIONAL CALL db.labels()
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.7 — parity scenario; OPTIONAL MATCH spelled
  # the same way in both dialects.
  @covers:optionalMatchStatement,optionalOperand,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] OPTIONAL MATCH parity
    Given an empty graph
    When executing query:
      """
      MATCH (a:Person)
      OPTIONAL MATCH (a)-[:KNOWS]->(b)
      RETURN a, b
      """
    Then no side effects
