#
# GQL ISO/IEC 39075:2024 conformance bootstrap — cyrs cy-inah
#
# Source: ISO/IEC 39075:2024 §16.7 (path pattern expression — node
# pattern).  A `nodePattern` is `( elementPatternFiller )`, where the
# filler combines an optional variable declaration, an optional label
# expression, and an optional property-map or WHERE predicate.  These
# scenarios pin the parser-accepted shapes of `nodePattern` /
# `elementPatternFiller` independently of any surrounding edge.
#

#encoding: utf-8

Feature: NodePattern1 - nodePattern shapes (ISO §16.7)

  # ISO/IEC 39075:2024 §16.7 — anonymous node, no filler.
  @covers:nodePattern,elementPattern,elementPatternFiller,pathPrimary,pathFactor,pathTerm,pathPatternExpression,pathPattern,pathPatternList,graphPattern,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [1] Anonymous node with empty filler
    Given an empty graph
    When executing query:
      """
      MATCH ()
      RETURN 1 AS one
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — named node, no label.
  @covers:nodePattern,elementPattern,elementPatternFiller,elementVariableDeclaration,elementVariable,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [2] Named node with variable only
    Given an empty graph
    When executing query:
      """
      MATCH (n)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 + §16.8 — single-label node via
  # `isLabelExpression` (COLON form).
  @covers:nodePattern,elementPattern,elementPatternFiller,elementVariableDeclaration,isLabelExpression,isOrColon,labelExpression,labelName,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [3] Labeled named node
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — anonymous node with label only.
  @covers:nodePattern,elementPattern,elementPatternFiller,isLabelExpression,isOrColon,labelExpression,labelName,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [4] Anonymous labeled node
    Given an empty graph
    When executing query:
      """
      MATCH (:Person)
      RETURN 1 AS one
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — property-map predicate (single key).
  @covers:nodePattern,elementPattern,elementPatternFiller,elementPatternPredicate,elementPropertySpecification,propertyKeyValuePairList,propertyKeyValuePair,propertyName,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [5] Anonymous node with single-property map
    Given an empty graph
    When executing query:
      """
      MATCH ({name: 'Ada'})
      RETURN 1 AS one
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 — combined: variable + label + properties.
  @covers:nodePattern,elementPattern,elementPatternFiller,elementVariableDeclaration,isLabelExpression,labelExpression,labelName,elementPatternPredicate,elementPropertySpecification,propertyKeyValuePairList,propertyKeyValuePair,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [6] Named labeled node with property map
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person {name: 'Ada', age: 30})
      RETURN n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §16.7 + §16.8 — multi-label colon-separated form
  # (each label is its own `labelExpression`).
  @covers:nodePattern,elementPatternFiller,isLabelExpression,labelExpression,labelName,simpleMatchStatement,matchStatement,returnStatement
  Scenario: [7] Multi-label colon-separated node
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person:Employee)
      RETURN n
      """
    Then no side effects
