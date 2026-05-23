#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-71t0
#
# Source: ISO/IEC 39075:2024 §14.13.3 (`groupByClause` /
# `groupingElementList` / `groupingElement`).  GQL exposes explicit
# `GROUP BY` in `returnStatementBody`; openCypher v9 has no surface for
# it (grouping is implicit on aggregating items).  These scenarios pin
# the GQL-distinct `RETURN ... GROUP BY` form so the parser change in
# cy-71t0 has a permanent home in the conformance corpus.
#

#encoding: utf-8

Feature: GroupBy1 - GQL GROUP BY in RETURN trailer

  # ISO/IEC 39075:2024 §14.13.3 — single grouping key.
  @covers:groupByClause,groupingElementList,groupingElement,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias,simpleMatchStatement,matchStatement
  Scenario: [1] RETURN ... GROUP BY single key
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.age AS age, count(*) AS c
      GROUP BY age
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.3 — multiple grouping keys, comma-separated.
  @covers:groupByClause,groupingElementList,groupingElement,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias,simpleMatchStatement,matchStatement
  Scenario: [2] RETURN ... GROUP BY multiple keys
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.country AS country, n.city AS city, count(*) AS c
      GROUP BY country, city
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.3 + §14.13.* — GROUP BY composes with the
  # rest of the return-body trailer (`groupByClause` precedes
  # `orderByClause` and `limitClause` per the returnStatementBody
  # production).
  @covers:groupByClause,groupingElementList,groupingElement,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias,orderByClause,sortSpecification,limitClause,simpleMatchStatement,matchStatement
  Scenario: [3] RETURN ... GROUP BY composed with ORDER BY and LIMIT
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.age AS age, count(*) AS c
      GROUP BY age
      ORDER BY age DESC
      LIMIT 10
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.3 — grouping element may be any value
  # expression, not just a bare alias reference.
  @covers:groupByClause,groupingElementList,groupingElement,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias,simpleMatchStatement,matchStatement
  Scenario: [4] RETURN ... GROUP BY expression
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.age / 10 AS decade, count(*) AS c
      GROUP BY n.age / 10
      """
    Then no side effects
