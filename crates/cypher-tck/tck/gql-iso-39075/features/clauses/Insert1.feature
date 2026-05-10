#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-0hj
#
# Source: ISO/IEC 39075:2024 §13.4 (insert statement).  GQL spells
# data-creation as `INSERT NODE` / `INSERT EDGE`; openCypher spells the
# equivalent as `CREATE`.  These scenarios pin the GQL-side surface so a
# future parser landing can flip them green without restating the cases.
#

#encoding: utf-8

Feature: Insert1 - INSERT NODE form (GQL-distinct)

  # ISO/IEC 39075:2024 §13.4.1 — `INSERT NODE` keyword sequence.
  Scenario: [1] Single labelled node insertion
    Given an empty graph
    When executing query:
      """
      INSERT NODE (n:Person {name: 'Ada'})
      """
    Then no side effects

  # ISO/IEC 39075:2024 §13.4.1 — multiple labels with `:`-separated form.
  Scenario: [2] Node insertion with multiple labels
    Given an empty graph
    When executing query:
      """
      INSERT NODE (n:Person:Employee {name: 'Bob'})
      """
    Then no side effects

  # ISO/IEC 39075:2024 §13.4.2 — `INSERT EDGE` directed form.
  Scenario: [3] Directed edge insertion between matched nodes
    Given an empty graph
    When executing query:
      """
      MATCH (a:Person {name: 'Ada'}), (b:Person {name: 'Bob'})
      INSERT EDGE (a)-[r:KNOWS {since: 2020}]->(b)
      """
    Then no side effects
