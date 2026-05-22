#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §14.10 (filter statement).  GQL provides a
# `FILTER` clause that operates on the working table after projection,
# distinct from `WHERE` which operates pre-projection.  openCypher has no
# direct analogue — `WITH ... WHERE` is the closest paraphrase.
#

#encoding: utf-8

Feature: Filter1 - FILTER clause (GQL-distinct)

  # ISO/IEC 39075:2024 §14.10 — post-projection FILTER.
  @covers:filterStatement,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] FILTER after RETURN-style projection
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.name AS name, n.age AS age
      FILTER age > 18
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.10 — FILTER chained after WITH.
  @covers:filterStatement,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] FILTER after WITH alias
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WITH n.name AS name, n.age AS age
      FILTER age >= 21
      RETURN name
      """
    Then no side effects
