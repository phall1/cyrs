#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-a6ci
#
# Source: ISO/IEC 39075:2024 §20.17 (list value constructor) and §21.2
# (`listLiteral`).  GQL list literals are bracketed enumerations of
# value expressions; the empty list is the degenerate case.
#

#encoding: utf-8

Feature: Lists1 - list value constructors and list literals

  # ISO/IEC 39075:2024 §20.17 — list constructor by enumeration.
  @covers:listLiteral,listValueConstructorByEnumeration,listElementList,listElement,generalLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [1] Integer list literal
    Given an empty graph
    When executing query:
      """
      RETURN [1, 2, 3] AS xs
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.17 — empty list literal (degenerate case).
  @covers:listLiteral,listValueConstructorByEnumeration,generalLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [2] Empty list literal
    Given an empty graph
    When executing query:
      """
      RETURN [] AS empty
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.17 — mixed-type list literal of expressions.
  @covers:listLiteral,listValueConstructorByEnumeration,listElementList,listElement,characterStringLiteral,generalLiteral,unsignedLiteral,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [3] String list literal
    Given an empty graph
    When executing query:
      """
      RETURN ['a', 'b', 'c'] AS letters
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.17 — nested list literal.
  @covers:listLiteral,listValueConstructorByEnumeration,listElementList,listElement,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [4] Nested list literal
    Given an empty graph
    When executing query:
      """
      RETURN [[1, 2], [3, 4]] AS matrix
      """
    Then no side effects
