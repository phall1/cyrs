#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-a6ci
#
# Source: ISO/IEC 39075:2024 §20.7 (case expression) and §20.9
# (aggregate function).  CASE comes in `searchedCase` (`CASE WHEN
# searchCondition THEN ...`) and `simpleCase` (`CASE operand WHEN
# value THEN ...`) forms.  Aggregations include `COUNT(*)`, generic
# `generalSetFunction` (`SUM`, `AVG`, `MIN`, `MAX`, `COLLECT_LIST`)
# and the `DISTINCT` set quantifier.
#

#encoding: utf-8

Feature: CaseAndAgg1 - CASE expressions and aggregate functions

  # ISO/IEC 39075:2024 §20.7 — searched-CASE in projection.
  @covers:caseExpression,searchedCase,searchedWhenClause,elseClause,result,resultExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [1] Searched CASE expression with ELSE
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN CASE WHEN n.age < 18 THEN 'minor' ELSE 'adult' END AS bucket
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.7 — simple-CASE with operand and WHEN values.
  @covers:caseExpression,simpleCase,simpleWhenClause,whenOperandList,whenOperand,caseOperand,elseClause,result,resultExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [2] Simple CASE with operand
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN CASE n.country
               WHEN 'US' THEN 1
               WHEN 'CA' THEN 2
               ELSE 0
             END AS code
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.9 — COUNT(*) aggregate.
  @covers:aggregateFunction,valueExpressionPrimary,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [3] COUNT(*) aggregate
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN COUNT(*) AS total
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.9 — generalSetFunction (SUM) over expression.
  @covers:aggregateFunction,generalSetFunction,generalSetFunctionType,valueExpressionPrimary,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [4] SUM aggregate over property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN SUM(n.balance) AS total
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.9 — AVG, MIN, MAX in a single projection.
  @covers:aggregateFunction,generalSetFunction,generalSetFunctionType,valueExpressionPrimary,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [5] AVG, MIN, MAX aggregates
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN AVG(n.age) AS mean, MIN(n.age) AS lo, MAX(n.age) AS hi
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.9 — DISTINCT setQuantifier inside generalSetFunction.
  @covers:aggregateFunction,generalSetFunction,generalSetFunctionType,setQuantifier,valueExpressionPrimary,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [6] COUNT DISTINCT aggregate
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN COUNT(DISTINCT n.country) AS countries
      """
    Then no side effects
