#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-gxda
#
# Source: ISO/IEC 39075:2024 §15.1 (composite query expression), §15.2
# (query conjunction).  A `compositeQueryExpression` chains linear query
# statements with a `queryConjunction` — a `setOperator` (`UNION`,
# `INTERSECT`, `EXCEPT`) optionally qualified by a `setQuantifier`
# (`ALL` / `DISTINCT`), or the `OTHERWISE` operator.  cyrs's GqlAligned
# dialect today accepts `UNION` and `UNION ALL` (with and without
# chaining); other set operators are not yet implemented and are
# excluded from this corpus.
#

#encoding: utf-8

Feature: Composite1 - UNION composite query conjunction

  # ISO/IEC 39075:2024 §15.2 — bare UNION between two linear queries.
  @covers:compositeQueryExpression,compositeQueryStatement,compositeQueryPrimary,queryConjunction,setOperator,simpleQueryStatement,simpleLinearQueryStatement,linearQueryStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [1] UNION of two linear queries
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person) RETURN n.name AS name
      UNION
      MATCH (n:Company) RETURN n.name AS name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — UNION ALL (setOperator + setQuantifier).
  @covers:compositeQueryExpression,compositeQueryStatement,compositeQueryPrimary,queryConjunction,setOperator,setQuantifier,simpleQueryStatement,linearQueryStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [2] UNION ALL preserves multiplicity
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person) RETURN n.name AS name
      UNION ALL
      MATCH (n:Company) RETURN n.name AS name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §15.1 — three-way left-associative composite chain.
  @covers:compositeQueryExpression,compositeQueryStatement,compositeQueryPrimary,queryConjunction,setOperator,simpleQueryStatement,linearQueryStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [3] Three-way UNION chain
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person) RETURN n.name AS name
      UNION
      MATCH (n:Company) RETURN n.name AS name
      UNION
      MATCH (n:Pet) RETURN n.name AS name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §15.1 / §16.4 — chain mixing UNION ALL and UNION
  # (the latter is set-distinct by default).
  @covers:compositeQueryExpression,compositeQueryStatement,compositeQueryPrimary,queryConjunction,setOperator,setQuantifier,linearQueryStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [4] Chained UNION ALL then UNION
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person) RETURN n.name AS name
      UNION ALL
      MATCH (n:Company) RETURN n.name AS name
      UNION
      MATCH (n:Pet) RETURN n.name AS name
      """
    Then no side effects
