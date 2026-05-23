#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-gxda
#
# Source: ISO/IEC 39075:2024 §14.3 (linear query statement / simple query
# statement), §14.5 (primitive query statement), §14.11.3 (optional
# match statement).  A `simpleLinearQueryStatement` is a sequence of
# `simpleQueryStatement`s, each of which is either a
# `primitiveQueryStatement` (e.g. `matchStatement`) or a
# `callQueryStatement`.  This file pins multi-statement linear-query
# composition: chained MATCHes, MATCH + OPTIONAL MATCH, and CALL +
# MATCH, terminating in a single `returnStatement`.
#

#encoding: utf-8

Feature: LinearQuery1 - multi-statement linear query composition

  # ISO/IEC 39075:2024 §14.3 — two MATCHes feeding one RETURN.
  @covers:simpleLinearQueryStatement,linearQueryStatement,simpleQueryStatement,primitiveQueryStatement,matchStatement,simpleMatchStatement,returnStatement,returnStatementBody,returnItemList,returnItem
  Scenario: [1] Two MATCH statements feeding a single RETURN
    Given an empty graph
    When executing query:
      """
      MATCH (a:Person)
      MATCH (b:Company)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.5 — MATCH + WHERE feeding another MATCH + WHERE.
  @covers:simpleLinearQueryStatement,linearQueryStatement,simpleQueryStatement,primitiveQueryStatement,matchStatement,simpleMatchStatement,whereClause,searchCondition,returnStatement,returnItem
  Scenario: [2] Two filtered MATCHes feeding a single RETURN
    Given an empty graph
    When executing query:
      """
      MATCH (a:Person) WHERE a.age > 18
      MATCH (b:Company) WHERE b.size > 100
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.11.3 — MATCH followed by OPTIONAL MATCH.
  @covers:simpleLinearQueryStatement,linearQueryStatement,simpleQueryStatement,primitiveQueryStatement,matchStatement,simpleMatchStatement,optionalMatchStatement,optionalOperand,returnStatement,returnItem
  Scenario: [3] MATCH then OPTIONAL MATCH
    Given an empty graph
    When executing query:
      """
      MATCH (a:Person)
      OPTIONAL MATCH (b:Company)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.3 — three MATCHes (long linear chain).
  @covers:simpleLinearQueryStatement,linearQueryStatement,simpleQueryStatement,primitiveQueryStatement,matchStatement,simpleMatchStatement,returnStatement,returnItemList
  Scenario: [4] Three MATCHes feeding one RETURN
    Given an empty graph
    When executing query:
      """
      MATCH (a:Person)
      MATCH (b:Company)
      MATCH (c:Pet)
      RETURN a, b, c
      """
    Then no side effects
