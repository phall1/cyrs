#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §10.4 (path selectors).  GQL formalises path
# search modes (`ANY SHORTEST`, `ALL SHORTEST`, `SHORTEST k`, `ANY`)
# with a uniform syntax slot in the path pattern.  openCypher exposes the
# same idea only via the `shortestPath()` / `allShortestPaths()` functions.
#

#encoding: utf-8

Feature: PathSelector1 - GQL path-selector syntax

  # ISO/IEC 39075:2024 §10.4.2 — `ANY SHORTEST` selector.
  @covers:pathPattern,pathPatternPrefix,pathSearchPrefix,shortestPathSearch,anyShortestPathSearch,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] ANY SHORTEST selector
    Given an empty graph
    When executing query:
      """
      MATCH p = ANY SHORTEST (a:Person)-[:KNOWS]->+(b:Person)
      RETURN p
      """
    Then no side effects

  # ISO/IEC 39075:2024 §10.4.2 — `ALL SHORTEST` selector.
  @covers:pathPattern,pathPatternPrefix,pathSearchPrefix,shortestPathSearch,allShortestPathSearch,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] ALL SHORTEST selector
    Given an empty graph
    When executing query:
      """
      MATCH p = ALL SHORTEST (a:Person)-[:KNOWS]->+(b:Person)
      RETURN p
      """
    Then no side effects

  # ISO/IEC 39075:2024 §10.4.2 — `SHORTEST k` selector.
  @covers:pathPattern,pathPatternPrefix,pathSearchPrefix,shortestPathSearch,countedShortestPathSearch,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] SHORTEST k selector with k=3
    Given an empty graph
    When executing query:
      """
      MATCH p = SHORTEST 3 (a:Person)-[:KNOWS]->+(b:Person)
      RETURN p
      """
    Then no side effects
