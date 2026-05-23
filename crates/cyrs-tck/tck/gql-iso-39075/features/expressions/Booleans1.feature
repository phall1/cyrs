#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-a6ci
#
# Source: ISO/IEC 39075:2024 §20.1 (value expression — boolean
# operators) and §21.2 (literal — `truthValue`).  Boolean value
# expressions support `AND`, `OR`, `XOR`, `NOT`, and the `IS [NOT]
# <truthValue>` predicate.
#

#encoding: utf-8

Feature: Booleans1 - boolean operators and truth-value predicates

  # ISO/IEC 39075:2024 §20.1 — conjunction of two comparisons.
  @covers:valueExpression,compOp,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] AND combinator
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.age >= 18 AND n.country = 'US'
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — disjunction of two comparisons.
  @covers:valueExpression,compOp,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] OR combinator
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.age < 13 OR n.age > 65
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — negation operator (`NOT`).
  @covers:valueExpression,compOp,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] NOT operator
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE NOT n.age < 18
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — exclusive-or operator (`XOR`).
  @covers:valueExpression,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] XOR operator
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.isMember XOR n.isGuest
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — boolean literal compared directly.
  @covers:valueExpression,compOp,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] Boolean literal in comparison
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.active = true
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — parenthesized boolean sub-expressions
  # combined with conjunction and disjunction.
  @covers:valueExpression,parenthesizedValueExpression,valueExpressionPrimary,compOp,whereClause,searchCondition,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [6] Parenthesized boolean subgroups
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE (n.age > 18 AND n.country = 'US') OR (n.role = 'admin')
      RETURN n
      """
    Then no side effects
