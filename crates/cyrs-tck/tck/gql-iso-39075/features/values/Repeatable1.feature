#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §10.6 (path-pattern quantifier modifiers).
# GQL adds `REPEATABLE ELEMENTS` and `DIFFERENT EDGES` qualifiers on
# variable-length path patterns to control element-uniqueness semantics.
# openCypher hard-codes `RELATIONSHIP_ISOMORPHISM` (no repeated edges) and
# offers no syntax for opting into other semantics.
#

#encoding: utf-8

Feature: Repeatable1 - REPEATABLE / DIFFERENT element semantics (GQL-distinct)

  # ISO/IEC 39075:2024 §10.6.3 — `REPEATABLE ELEMENTS` qualifier.
  Scenario: [1] Variable-length path with REPEATABLE ELEMENTS
    Given an empty graph
    When executing query:
      """
      MATCH REPEATABLE ELEMENTS (a)-[:KNOWS]->{1,5}(b)
      RETURN a, b
      """
    Then no side effects

  # ISO/IEC 39075:2024 §10.6.3 — `DIFFERENT EDGES` qualifier.
  Scenario: [2] Variable-length path with DIFFERENT EDGES
    Given an empty graph
    When executing query:
      """
      MATCH DIFFERENT EDGES (a)-[:KNOWS]->{1,5}(b)
      RETURN a, b
      """
    Then no side effects
