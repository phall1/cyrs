#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-a6ci
#
# Source: ISO/IEC 39075:2024 §20.1 (value expression) and §20.22 (numeric
# value function).  Numeric value expressions support unary sign, the
# multiplicative operators (`*`, `/`), and the additive operators (`+`,
# `-`).  §20.22 adds `ABS(...)` and `MOD(..., ...)` as numeric-valued
# functions.
#

#encoding: utf-8

Feature: Arithmetic1 - numeric arithmetic operators and functions

  # ISO/IEC 39075:2024 §20.1 — additive operator in projection.
  @covers:valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [1] Addition in RETURN projection
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.age + 1 AS nextAge
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — subtraction with parenthesized sub-expr.
  @covers:valueExpression,parenthesizedValueExpression,valueExpressionPrimary,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [2] Subtraction with parentheses
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN (n.age - 5) AS lower
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — multiplicative operators.
  @covers:valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [3] Multiplication and division
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n.score * 2 AS doubled, n.score / 4 AS quartered
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — unary minus (signed expression).
  @covers:valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [4] Unary minus on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN -n.balance AS negated
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — ABS numeric value function.
  @covers:absoluteValueExpression,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [5] ABS numeric value function
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN ABS(n.balance) AS magnitude
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — mixed additive and multiplicative
  # operators in a single expression (precedence + nesting).
  @covers:valueExpression,parenthesizedValueExpression,valueExpressionPrimary,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [6] Mixed arithmetic with precedence
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN (n.x + 1) * (n.y - 2) AS score
      """
    Then no side effects
