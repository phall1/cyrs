#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-gxda
#
# Source: ISO/IEC 39075:2024 §14.9 (order by and page statement),
# §16.18 (limit clause), §16.19 (offset clause).  GQL's page statement
# permits `OFFSET k` / `SKIP k` and `LIMIT k` as a tail on a result.
# Per §16.19 `SKIP` is a `offsetSynonym` for `OFFSET`.  cyrs's GqlAligned
# dialect today accepts the `SKIP` spelling; the `OFFSET` keyword is
# not yet wired up and is excluded from this corpus accordingly.
#

#encoding: utf-8

Feature: Page1 - LIMIT and SKIP/OFFSET pagination

  # ISO/IEC 39075:2024 §16.18 — LIMIT alone.
  @covers:limitClause,orderByAndPageStatement,primitiveResultStatement,returnStatement,nonNegativeIntegerSpecification,simpleMatchStatement,matchStatement
  Scenario: [1] LIMIT alone
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      LIMIT 10
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.19 — SKIP (offsetSynonym) alone.
  @covers:offsetClause,offsetSynonym,orderByAndPageStatement,primitiveResultStatement,returnStatement,nonNegativeIntegerSpecification,simpleMatchStatement,matchStatement
  Scenario: [2] SKIP alone (offsetSynonym)
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      SKIP 5
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.9 — SKIP followed by LIMIT (offset + limit).
  @covers:offsetClause,offsetSynonym,limitClause,orderByAndPageStatement,primitiveResultStatement,returnStatement,nonNegativeIntegerSpecification,simpleMatchStatement,matchStatement
  Scenario: [3] SKIP then LIMIT
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      SKIP 5
      LIMIT 10
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.9 — ORDER BY then OFFSET then LIMIT trailing
  # a return: the full orderByAndPageStatement shape.
  @covers:orderByClause,sortSpecificationList,sortSpecification,offsetClause,offsetSynonym,limitClause,orderByAndPageStatement,primitiveResultStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [4] ORDER BY then SKIP then LIMIT
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.name AS name
      ORDER BY name
      SKIP 5
      LIMIT 10
      """
    Then no side effects
