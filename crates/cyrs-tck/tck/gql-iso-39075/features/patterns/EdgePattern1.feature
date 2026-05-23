#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-inah
#
# Source: ISO/IEC 39075:2024 §16.7 (path pattern expression — edge
# pattern).  `edgePattern` resolves to either `fullEdgePattern` (the
# bracketed forms `-[…]->`, `<-[…]-`, `-[…]-`) or `abbreviatedEdgePattern`
# (the bracketless forms `-->`, `<--`, `--`).  Each direction variant is
# a distinct production; these scenarios pin the parser-accepted shapes.
#

#encoding: utf-8

Feature: EdgePattern1 - edgePattern direction variants (ISO §16.7)

  # ISO/IEC 39075:2024 §16.7 — full edge, right-pointing, typed + named.
  @covers:edgePattern,fullEdgePattern,fullEdgePointingRight,elementPatternFiller,elementVariableDeclaration,isLabelExpression,labelExpression,labelName,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Right-pointing full edge with type and variable
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r:KNOWS]->(b)
      RETURN r
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — full edge, left-pointing.
  @covers:edgePattern,fullEdgePattern,fullEdgePointingLeft,elementPatternFiller,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Left-pointing full edge with type and variable
    Given an empty graph
    When executing query:
      """
      MATCH (a)<-[r:KNOWS]-(b)
      RETURN r
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — full edge, undirected.
  @covers:edgePattern,fullEdgePattern,fullEdgeUndirected,elementPatternFiller,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] Undirected full edge with type and variable
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r:KNOWS]-(b)
      RETURN r
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — abbreviated (bracketless) right edge.
  @covers:edgePattern,abbreviatedEdgePattern,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] Abbreviated right-pointing edge
    Given an empty graph
    When executing query:
      """
      MATCH (a)-->(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — abbreviated (bracketless) left edge.
  @covers:edgePattern,abbreviatedEdgePattern,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] Abbreviated left-pointing edge
    Given an empty graph
    When executing query:
      """
      MATCH (a)<--(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — abbreviated (bracketless) undirected edge.
  @covers:edgePattern,abbreviatedEdgePattern,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [6] Abbreviated undirected edge
    Given an empty graph
    When executing query:
      """
      MATCH (a)--(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — full edge, untyped, anonymous variable.
  @covers:edgePattern,fullEdgePattern,fullEdgePointingRight,elementPatternFiller,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [7] Full edge with empty filler
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[]->(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — full edge, unlabeled, with variable.
  @covers:edgePattern,fullEdgePattern,fullEdgePointingRight,elementPatternFiller,elementVariableDeclaration,elementVariable,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [8] Full edge with variable but no label
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r]->(b)
      RETURN r
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — full edge with type AND property map.
  @covers:edgePattern,fullEdgePattern,fullEdgePointingRight,elementPatternFiller,elementVariableDeclaration,isLabelExpression,labelExpression,labelName,elementPatternPredicate,elementPropertySpecification,propertyKeyValuePairList,propertyKeyValuePair,propertyName,nodePattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [9] Right edge with type and property map
    Given an empty graph
    When executing query:
      """
      MATCH (a)-[r:KNOWS {since: 2020}]->(b)
      RETURN r
      """
    Then no side effects
