#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-p3cl
#
# Source: ISO/IEC 39075:2024 §16.4 (`labelExpression`).  A node-pattern
# label expression is built from label names with the operators
# `&` (conjunction), `|` (disjunction), `!` (negation), the wildcard
# `%`, and explicit parenthesisation.  These scenarios pin the
# parser-accepted shapes that landed in cy-p3cl.
#
# The `|` symbol is also the relationship-type-disjunction operator
# inside `edgePattern` (§16.6); these scenarios only exercise the
# node-pattern label position so the two surfaces stay disambiguated.
#

#encoding: utf-8

Feature: LabelExpr1 - GQL labelExpression operators (ISO §16.4)

  # ISO/IEC 39075:2024 §16.4 — conjunction.
  @covers:labelExpression,nodePattern,elementPattern,elementPatternFiller,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Conjunction A & B
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person & Employee)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — disjunction in node position.
  @covers:labelExpression,nodePattern,elementPattern,elementPatternFiller,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Disjunction A | B
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person | Robot)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — prefix negation.
  @covers:labelExpression,nodePattern,elementPattern,elementPatternFiller,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] Negation !A
    Given an empty graph
    When executing query:
      """
      MATCH (n:!Archived)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — wildcard `%` (any label).
  @covers:labelExpression,nodePattern,elementPattern,elementPatternFiller,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] Wildcard %
    Given an empty graph
    When executing query:
      """
      MATCH (n:%)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — parenthesised compound, mixed
  # precedence (disjunction inside, conjunction outside).
  @covers:labelExpression,nodePattern,elementPattern,elementPatternFiller,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] Parenthesised compound (A | B) & C
    Given an empty graph
    When executing query:
      """
      MATCH (n:(Person | Robot) & Active)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.4 — combined operators and explicit
  # parentheses around negation.
  @covers:labelExpression,nodePattern,elementPattern,elementPatternFiller,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [6] Negation inside conjunction !A & B
    Given an empty graph
    When executing query:
      """
      MATCH (n:!Archived & Active)
      RETURN n
      """
    Then no side effects
