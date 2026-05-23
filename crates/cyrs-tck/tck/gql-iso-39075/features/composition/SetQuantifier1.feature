#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-gxda
#
# Source: ISO/IEC 39075:2024 §14.13 (return statement), §16.4 (set
# quantifier).  GQL admits an explicit `setQuantifier` (`DISTINCT` or
# `ALL`) immediately after `RETURN`.  `RETURN ALL` is the default
# multiset semantics; `RETURN DISTINCT` deduplicates the projection.
#

#encoding: utf-8

Feature: SetQuantifier1 - DISTINCT and ALL on result projection

  # ISO/IEC 39075:2024 §16.4 — `RETURN DISTINCT` projection.
  @covers:setQuantifier,returnStatement,returnStatementBody,returnItem,returnItemList,returnItemAlias,simpleMatchStatement,matchStatement
  Scenario: [1] RETURN DISTINCT
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN DISTINCT n.name AS name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13 — `RETURN ALL` explicit multiset (default).
  @covers:setQuantifier,returnStatement,returnStatementBody,returnItem,simpleMatchStatement,matchStatement
  Scenario: [2] RETURN ALL explicit multiset
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN ALL n.name AS name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — DISTINCT projection composed with
  # ORDER BY / LIMIT, exercising the full result-statement shape.
  @covers:setQuantifier,orderByClause,sortSpecification,limitClause,orderByAndPageStatement,primitiveResultStatement,returnStatement,returnStatementBody,returnItem,simpleMatchStatement,matchStatement
  Scenario: [3] RETURN DISTINCT with ORDER BY and LIMIT
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN DISTINCT n.name AS name
      ORDER BY name
      LIMIT 10
      """
    Then no side effects
