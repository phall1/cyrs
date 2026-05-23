#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-a6ci
#
# Source: ISO/IEC 39075:2024 §19 (search condition and predicate).  The
# predicate surface covers comparison (`=`, `<>`, `<`, `>`, `<=`, `>=`),
# the null predicate (`IS [NOT] NULL`, §19.5), and the existence subquery
# predicate (`EXISTS { ... }`, §19.4).
#

#encoding: utf-8

Feature: Predicates1 - comparison, null, and exists predicates

  # ISO/IEC 39075:2024 §19.3 — equality comparison via compOp.
  @covers:compOp,valueExpression,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Equality comparison in WHERE
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.name = 'Alice'
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §19.3 — inequality, less-than, greater-than chain.
  @covers:compOp,valueExpression,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Inequality and ordering comparisons
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.age <> 0 AND n.age < 100 AND n.age >= 18
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §19.5 — IS NULL.
  @covers:nullPredicate,nullPredicatePart2,predicate,valueExpression,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] IS NULL predicate
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.middleName IS NULL
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §19.5 — IS NOT NULL.
  @covers:nullPredicate,nullPredicatePart2,predicate,valueExpression,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] IS NOT NULL predicate
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.email IS NOT NULL
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §19.4 — EXISTS subquery predicate.
  @covers:existsPredicate,predicate,valueExpression,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] EXISTS pattern subquery
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE EXISTS { MATCH (n)-[:KNOWS]->(:Person) }
      RETURN n
      """
    Then no side effects
