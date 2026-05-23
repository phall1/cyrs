#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-z0x8
#
# Source: ISO/IEC 39075:2024 §14.13.6 (`sortSpecification` /
# `nullOrdering`) and §14.13.7 (`offsetClause` / `offsetSynonym`).
# The companion `Page1.feature` (cy-gxda) pinned `LIMIT` and the
# `SKIP` spelling of the offset synonym.  This file pins the
# explicit `OFFSET k` spelling and the `NULLS FIRST` / `NULLS LAST`
# sort-spec trailer added in cyrs cy-z0x8.
#

#encoding: utf-8

Feature: Page2 - GQL OFFSET synonym and NULLS FIRST/LAST sort trailer

  # ISO/IEC 39075:2024 §14.13.7 — OFFSET (the other offsetSynonym).
  @covers:offsetClause,offsetSynonym,orderByAndPageStatement,primitiveResultStatement,returnStatement,nonNegativeIntegerSpecification,simpleMatchStatement,matchStatement
  Scenario: [1] OFFSET alone (offsetSynonym)
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      OFFSET 10
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.7 + §14.13.* — LIMIT followed by
  # OFFSET (the explicit ANSI spelling), composed with a RETURN.
  @covers:offsetClause,offsetSynonym,limitClause,orderByAndPageStatement,primitiveResultStatement,returnStatement,nonNegativeIntegerSpecification,simpleMatchStatement,matchStatement
  Scenario: [2] LIMIT then OFFSET
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      OFFSET 5
      LIMIT 10
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.6 — sortSpecification with a
  # `nullOrdering` trailer (NULLS FIRST), no orderingSpecification.
  @covers:nullOrdering,orderByClause,sortSpecificationList,sortSpecification,sortKey,orderByAndPageStatement,primitiveResultStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [3] ORDER BY x NULLS FIRST
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.name AS name
      ORDER BY name NULLS FIRST
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.6 — sortSpecification with both an
  # orderingSpecification (`DESC`) and a `nullOrdering` (`NULLS LAST`).
  @covers:nullOrdering,orderingSpecification,orderByClause,sortSpecificationList,sortSpecification,sortKey,orderByAndPageStatement,primitiveResultStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [4] ORDER BY x DESC NULLS LAST
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.age AS age
      ORDER BY age DESC NULLS LAST
      """
    Then no side effects
