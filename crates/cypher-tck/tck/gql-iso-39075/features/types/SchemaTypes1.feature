#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §6 (data types) and §6.5 (typed values).
# GQL provides explicit type assertions via `IS TYPED` (or shorthand `::`)
# and a richer set of named scalar / structured types than openCypher.
#

#encoding: utf-8

Feature: SchemaTypes1 - Type assertions and named GQL types

  # ISO/IEC 39075:2024 §6.5.2 — `IS TYPED` predicate.
  Scenario: [1] IS TYPED predicate
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.age IS TYPED INTEGER
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §6.5.2 — `::` typed-value shorthand.
  Scenario: [2] Double-colon type assertion
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.age :: INTEGER > 18
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §6.2 — GQL `ZONED DATETIME` named type.
  Scenario: [3] ZONED DATETIME literal in cast
    Given an empty graph
    When executing query:
      """
      RETURN '2024-01-01T00:00:00Z' :: ZONED DATETIME AS ts
      """
    Then no side effects
