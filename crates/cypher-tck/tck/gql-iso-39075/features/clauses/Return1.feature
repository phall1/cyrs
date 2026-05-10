#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §14.13 (return statement).  GQL distinguishes
# `RETURN ALL` (multiset semantics, default) from `RETURN DISTINCT` more
# explicitly than openCypher, and supports `EXCLUDE <field>` to project all
# bindings minus an exclusion list.
#

#encoding: utf-8

Feature: Return1 - RETURN clause GQL extensions

  # ISO/IEC 39075:2024 §14.13.2 — explicit `RETURN ALL` keyword.
  Scenario: [1] RETURN ALL explicit multiset
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN ALL n.name AS name
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.4 — `EXCLUDE` field projection.
  Scenario: [2] RETURN with EXCLUDE field
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n EXCLUDE password
      """
    Then no side effects

  # ISO/IEC 39075:2024 §14.13.5 — `RETURN *` parity (also in openCypher).
  Scenario: [3] RETURN wildcard
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN *
      """
    Then no side effects
