#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-a6ci
#
# Source: ISO/IEC 39075:2024 §21.2 (literal).  Covers the unsigned
# numeric literal forms (`unsignedDecimalInteger`, `exactNumericLiteral`,
# `approximateNumericLiteral`), `characterStringLiteral`, `nullLiteral`,
# and `generalLiteral` reaching boolean / string alternatives.
#

#encoding: utf-8

Feature: Literals1 - numeric, string, null, boolean literals

  # ISO/IEC 39075:2024 §21.2 — unsigned decimal integer literal.
  @covers:unsignedDecimalInteger,unsignedInteger,exactNumericLiteral,unsignedNumericLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [1] Integer literal projection
    Given an empty graph
    When executing query:
      """
      RETURN 42 AS answer
      """
    Then no side effects

  # ISO/IEC 39075:2024 §21.2 — exact decimal with fractional part.
  @covers:exactNumericLiteral,unsignedNumericLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [2] Decimal literal with fractional part
    Given an empty graph
    When executing query:
      """
      RETURN 3.14 AS pi
      """
    Then no side effects

  # ISO/IEC 39075:2024 §21.2 — approximate (scientific-notation) numeric.
  @covers:approximateNumericLiteral,unsignedNumericLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [3] Scientific notation numeric literal
    Given an empty graph
    When executing query:
      """
      RETURN 6.022e23 AS avogadro
      """
    Then no side effects

  # ISO/IEC 39075:2024 §21.2 — single-quoted character string literal.
  @covers:characterStringLiteral,generalLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [4] String literal projection
    Given an empty graph
    When executing query:
      """
      RETURN 'hello, world' AS greeting
      """
    Then no side effects

  # ISO/IEC 39075:2024 §21.2 — `NULL` literal in projection.
  @covers:nullLiteral,generalLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [5] NULL literal projection
    Given an empty graph
    When executing query:
      """
      RETURN NULL AS nothing
      """
    Then no side effects

  # ISO/IEC 39075:2024 §21.2 — boolean literal reaches generalLiteral.
  @covers:generalLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [6] Boolean literal projection
    Given an empty graph
    When executing query:
      """
      RETURN true AS yes, false AS no
      """
    Then no side effects
