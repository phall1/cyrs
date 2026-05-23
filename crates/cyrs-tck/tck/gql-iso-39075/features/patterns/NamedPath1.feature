#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-inah
#
# Source: ISO/IEC 39075:2024 §16.4 (graph pattern — `pathPattern` with
# `pathVariableDeclaration`).  A `pathPattern` may bind its
# `pathPatternExpression` to a name via `pathVariable EQUALS_OPERATOR`.
# The bound name then refers to the whole matched path and can appear
# in the projection.
#

#encoding: utf-8

Feature: NamedPath1 - pathVariableDeclaration (ISO §16.4)

  # ISO/IEC 39075:2024 §16.4 — single-hop named path.
  @covers:pathPattern,pathVariableDeclaration,pathVariable,pathPatternExpression,pathTerm,pathFactor,pathPrimary,elementPattern,nodePattern,edgePattern,fullEdgePointingRight,pathPatternList,graphPattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Named single-hop path
    Given an empty graph
    When executing query:
      """
      MATCH p = (a)-[r]->(b)
      RETURN p
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — named path over a multi-hop concatenation.
  @covers:pathPattern,pathVariableDeclaration,pathVariable,pathPatternExpression,pathTerm,pathFactor,pathPrimary,nodePattern,edgePattern,fullEdgePointingRight,pathPatternList,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Named multi-hop path
    Given an empty graph
    When executing query:
      """
      MATCH p = (a)-[r1]->(b)-[r2]->(c)
      RETURN p
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — two independently-named paths in the
  # same `pathPatternList`.
  @covers:pathPatternList,pathPattern,pathVariableDeclaration,pathVariable,pathPatternExpression,nodePattern,edgePattern,fullEdgePointingRight,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] Two named paths in one MATCH
    Given an empty graph
    When executing query:
      """
      MATCH p = (a)-[r]->(b), q = (c)-[s]->(d)
      RETURN p, q
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — named path over a labelled-and-typed hop.
  @covers:pathPattern,pathVariableDeclaration,pathVariable,pathPatternExpression,nodePattern,elementPatternFiller,isLabelExpression,labelExpression,labelName,edgePattern,fullEdgePointingRight,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] Named path over labelled endpoints
    Given an empty graph
    When executing query:
      """
      MATCH p = (a:Person)-[r:KNOWS]->(b:Person)
      RETURN p
      """
    Then no side effects
