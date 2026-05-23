#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-dwem
#
# Source: ISO/IEC 39075:2024 §20.1 (`booleanValueExpression` /
# `truthValuePredicatePart2` / `truthValue`).  GQL exposes the
# truth-value predicate as a postfix tail on an expression:
#
#     <expr> IS [NOT] ( TRUE | FALSE | UNKNOWN )
#
# Prior to cy-dwem the cyrs parser rejected every form (`IS TRUE`,
# `IS NOT FALSE`, `IS UNKNOWN`, …) — `IS` was hard-wired to expect
# either `NULL` or `TYPED <Type>`.  These scenarios pin the GQL-distinct
# truthValuePredicate surface so the parser change in cy-dwem has a
# permanent home in the conformance corpus.
#
# `truthValuePredicate` / `truthValuePredicatePart2` are not enumerated
# in the OpenGQL ANTLR `rules.json` snapshot the harness reads (the
# grammar inlines them into `predicate`), so the `@covers:` tag set
# stops at `truthValue` — the leaf production that lists `TRUE | FALSE
# | UNKNOWN` and is reached by every scenario below.
#

#encoding: utf-8

Feature: TruthValue1 - GQL truth-value predicate (IS TRUE / FALSE / UNKNOWN)

  # ISO/IEC 39075:2024 §20.1 — `<expr> IS TRUE`.
  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,simpleMatchStatement,matchStatement
  Scenario: [1] IS TRUE on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.flag IS TRUE
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — `<expr> IS FALSE`.
  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,simpleMatchStatement,matchStatement
  Scenario: [2] IS FALSE on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.flag IS FALSE
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — `<expr> IS UNKNOWN` (the GQL-distinct
  # truthValue tail; openCypher v9 has no surface for it because its
  # boolean logic is two-valued).
  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,simpleMatchStatement,matchStatement
  Scenario: [3] IS UNKNOWN on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.flag IS UNKNOWN
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — negated forms.
  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,simpleMatchStatement,matchStatement
  Scenario: [4] IS NOT TRUE on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.flag IS NOT TRUE
      RETURN n
      """
    Then no side effects

  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,simpleMatchStatement,matchStatement
  Scenario: [5] IS NOT FALSE on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.flag IS NOT FALSE
      RETURN n
      """
    Then no side effects

  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,simpleMatchStatement,matchStatement
  Scenario: [6] IS NOT UNKNOWN on a property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.flag IS NOT UNKNOWN
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.1 — the predicate composes with the rest of
  # the boolean expression algebra (`AND`, `OR`).
  @covers:truthValue,returnStatement,returnStatementBody,returnItemList,returnItem,searchCondition,simpleMatchStatement,matchStatement
  Scenario: [7] truthValue predicate composed with AND
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      WHERE n.a IS TRUE AND n.b IS NOT FALSE
      RETURN n
      """
    Then no side effects
