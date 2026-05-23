#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-inah
#
# Source: ISO/IEC 39075:2024 §16.11 (graph pattern quantifier).  A
# `graphPatternQuantifier` attaches to a `pathPrimary` (the
# `pfQuantifiedPathPrimary` alternative of `pathFactor`) and is one of:
# `*`, `+`, `{n}` (fixed), or `{lower?,upper?}` (general).  These
# scenarios pin the quantifier shapes the parser accepts on a typed
# edge primary.
#

#encoding: utf-8

Feature: Quantifier1 - graphPatternQuantifier shapes (ISO §16.11)

  # ISO/IEC 39075:2024 §16.11 — `+` quantifier on a right edge.
  @covers:graphPatternQuantifier,pathFactor,pathPrimary,edgePattern,fullEdgePattern,fullEdgePointingRight,nodePattern,pathTerm,pathPatternExpression,pathPattern,pathPatternList,graphPattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Plus quantifier on right edge
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[:KNOWS]->+(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.11 — fixed-length `{n}` quantifier.
  @covers:graphPatternQuantifier,fixedQuantifier,pathFactor,pathPrimary,edgePattern,fullEdgePointingRight,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Fixed quantifier with k=3
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[:KNOWS]->{3}(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.11 — general `{lower,upper}` quantifier.
  @covers:graphPatternQuantifier,generalQuantifier,lowerBound,upperBound,pathFactor,pathPrimary,edgePattern,fullEdgePointingRight,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] General quantifier with explicit bounds
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[:KNOWS]->{1,5}(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.11 — general quantifier, lower-bound only.
  @covers:graphPatternQuantifier,generalQuantifier,lowerBound,pathFactor,pathPrimary,edgePattern,fullEdgePointingRight,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] General quantifier with lower bound only
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[:KNOWS]->{2,}(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.11 — general quantifier, upper-bound only.
  @covers:graphPatternQuantifier,generalQuantifier,upperBound,pathFactor,pathPrimary,edgePattern,fullEdgePointingRight,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] General quantifier with upper bound only
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[:KNOWS]->{,5}(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.11 — quantifier on a left-pointing edge.
  @covers:graphPatternQuantifier,generalQuantifier,lowerBound,upperBound,pathFactor,pathPrimary,edgePattern,fullEdgePointingLeft,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [6] General quantifier on left-pointing edge
    Given an empty graph
    When executing query:
      """
      MATCH (a)<-[r:KNOWS]-{1,3}(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.11 — quantifier on an undirected edge.
  @covers:graphPatternQuantifier,generalQuantifier,lowerBound,upperBound,pathFactor,pathPrimary,edgePattern,fullEdgeUndirected,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [7] General quantifier on undirected edge
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r:KNOWS]-{1,3}(b)
      RETURN a, b
      """
    Then no side effects
