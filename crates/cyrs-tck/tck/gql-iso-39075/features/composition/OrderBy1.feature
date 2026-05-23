#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-gxda
#
# Source: ISO/IEC 39075:2024 §14.9 (order by and page statement), §16.17
# (sort specification list).  GQL admits `ORDER BY` as part of the
# `orderByAndPageStatement` that may trail a `returnStatement`, with sort
# keys taking an optional `orderingSpecification` (`ASC` / `ASCENDING` /
# `DESC` / `DESCENDING`).  This file pins the productions that are
# accepted by cyrs's GqlAligned dialect today.
#

#encoding: utf-8

Feature: OrderBy1 - ORDER BY clause and sort specifications

  # ISO/IEC 39075:2024 §14.9 — ORDER BY with implicit ascending sort.
  @covers:orderByClause,sortSpecificationList,sortSpecification,sortKey,orderByAndPageStatement,primitiveResultStatement,returnStatement,returnStatementBody,returnItem,simpleMatchStatement,matchStatement
  Scenario: [1] ORDER BY single sort key (implicit ASC)
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.name AS name
      ORDER BY name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.17 — ORDER BY DESC.
  @covers:orderByClause,sortSpecificationList,sortSpecification,sortKey,orderingSpecification,orderByAndPageStatement,primitiveResultStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [2] ORDER BY single sort key DESC
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.name AS name
      ORDER BY name DESC
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.17 — multi-key ORDER BY mixing ASC and DESC.
  @covers:orderByClause,sortSpecificationList,sortSpecification,sortKey,orderingSpecification,orderByAndPageStatement,primitiveResultStatement,returnStatement,returnItemList,simpleMatchStatement,matchStatement
  Scenario: [3] ORDER BY multiple keys mixing ASC and DESC
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.name AS name, n.age AS age
      ORDER BY age DESC, name ASC
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.17 — ORDER BY using ASCENDING/DESCENDING spelling.
  @covers:orderByClause,sortSpecificationList,sortSpecification,orderingSpecification,orderByAndPageStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [4] ORDER BY ASCENDING and DESCENDING spellings
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.a AS a, n.b AS b
      ORDER BY a ASCENDING, b DESCENDING
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.17 — ORDER BY over a compound sort-key expression.
  @covers:orderByClause,sortSpecification,sortKey,aggregatingValueExpression,orderByAndPageStatement,returnStatement,simpleMatchStatement,matchStatement
  Scenario: [5] ORDER BY arithmetic expression
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      ORDER BY n.a + n.b
      """
    Then no side effects
