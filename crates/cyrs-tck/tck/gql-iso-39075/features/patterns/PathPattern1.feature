#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-inah
#
# Source: ISO/IEC 39075:2024 §16.4 (graph pattern — `pathPatternList`)
# and §16.7 (`pathPattern`, `pathPatternExpression`).  These scenarios
# exercise multi-element paths: hop concatenation inside one
# `pathPatternExpression`, comma-separated `pathPattern`s in one
# `pathPatternList`, and the `pathVariableDeclaration` form that binds
# a path to a name.
#

#encoding: utf-8

Feature: PathPattern1 - pathPatternList & path concatenation (ISO §16.4, §16.7)

  # ISO/IEC 39075:2024 §16.7 — two hops in one `pathPatternExpression`.
  @covers:pathPatternExpression,pathTerm,pathFactor,pathPrimary,elementPattern,nodePattern,edgePattern,fullEdgePointingRight,pathPattern,pathPatternList,graphPattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Two-hop right-edge concatenation
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r1]->(b)-[r2]->(c)
      RETURN a, c
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — mixed direction concatenation.
  @covers:pathPatternExpression,pathTerm,pathFactor,pathPrimary,elementPattern,nodePattern,edgePattern,fullEdgePointingRight,fullEdgePointingLeft,pathPattern,pathPatternList,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Mixed-direction two-hop path
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r1]->(b)<-[r2]-(c)
      RETURN a, c
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — two comma-separated path patterns.
  @covers:pathPatternList,pathPattern,pathPatternExpression,pathTerm,pathFactor,pathPrimary,elementPattern,nodePattern,edgePattern,fullEdgePointingRight,graphPattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] Comma-separated path pattern list
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r1]->(b), (c)-[r2]->(d)
      RETURN a, c
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — three node-only patterns in one list.
  @covers:pathPatternList,pathPattern,pathPatternExpression,pathTerm,pathFactor,pathPrimary,elementPattern,nodePattern,graphPattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] Three comma-separated single-node patterns
    Given an empty graph
    When executing query:
      """
      MATCH (a), (b), (c)
      RETURN a, b, c
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — three-hop concatenation.
  @covers:pathPatternExpression,pathTerm,pathFactor,pathPrimary,elementPattern,nodePattern,edgePattern,abbreviatedEdgePattern,pathPattern,pathPatternList,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] Three-hop abbreviated-edge path
    Given an empty graph
    When executing query:
      """
      MATCH (a)-->(b)-->(c)-->(d)
      RETURN a, d
      """
    Then no side effects
