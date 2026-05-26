#
# GQL ISO/IEC 39075:2024 conformance corpus — cyrs cy-numf
#
# Source: ISO/IEC 39075:2024 §20.22 (numeric value function).  Covers
# the GQL `numericValueFunction` alternatives that are spelled as
# `IDENT(args)` and therefore lex as identifier-shaped function calls
# in cyrs's GqlAligned dialect:
#
#   CEILING / CEIL, FLOOR, EXP, LN, LOG10, LOG(base, value),
#   POWER(base, exp), SQRT, SIN / COS / TAN (trigonometricFunction),
#   CARDINALITY / SIZE (cardinalityExpression), CHAR_LENGTH /
#   CHARACTER_LENGTH (charLengthExpression), OCTET_LENGTH /
#   BYTE_LENGTH (byteLengthExpression), PATH_LENGTH
#   (pathLengthExpression).
#
# `MOD(x, y)` (modulusExpression) is intentionally not exercised here:
# `MOD` is reserved at the lexer level in cyrs, so the function-call
# form needs a dedicated parser path before a scenario can succeed.
# Tracked as a follow-up parser bead.
#

#encoding: utf-8

Feature: NumericFunctions1 - GQL numeric value functions

  # ISO/IEC 39075:2024 §20.22 — CEILING(x): round towards +infinity.
  @covers:ceilingFunction,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [1] CEILING numeric value function
    Given an empty graph
    When executing query:
      """
      MATCH (n:Item)
      RETURN CEILING(n.weight) AS rounded_up
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — CEIL synonym for CEILING.
  @covers:ceilingFunction,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [2] CEIL synonym for CEILING
    Given an empty graph
    When executing query:
      """
      MATCH (n:Item)
      RETURN CEIL(n.weight) AS rounded_up
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — FLOOR(x): round towards -infinity.
  @covers:floorFunction,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [3] FLOOR numeric value function
    Given an empty graph
    When executing query:
      """
      MATCH (n:Item)
      RETURN FLOOR(n.weight) AS rounded_down
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — EXP(x): natural exponential.
  @covers:exponentialFunction,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [4] EXP numeric value function
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN EXP(n.t) AS growth
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — LN(x): natural logarithm.
  @covers:naturalLogarithm,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [5] LN natural logarithm
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN LN(n.t) AS log_e
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — LOG10(x): base-10 logarithm.
  @covers:commonLogarithm,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [6] LOG10 common logarithm
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN LOG10(n.t) AS log_base_10
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — LOG(base, x): logarithm of x in given base.
  @covers:generalLogarithmFunction,generalLogarithmBase,generalLogarithmArgument,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [7] LOG general logarithm with explicit base
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN LOG(2, n.t) AS log_base_2
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — POWER(base, exp).
  @covers:powerFunction,numericValueExpressionBase,numericValueExpressionExponent,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [8] POWER raises base to exponent
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN POWER(n.base, 3) AS cubed
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — SQRT(x): square root.
  @covers:squareRoot,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [9] SQRT square root
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN SQRT(n.area) AS side
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — trigonometric functions
  # (SIN/COS/TAN and their hyperbolic + inverse cousins).  cyrs treats
  # the function name as a regular identifier; the harness counts the
  # `trigonometricFunction` production once across these.
  @covers:trigonometricFunction,trigonometricFunctionName,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [10] SIN COS TAN trigonometric functions
    Given an empty graph
    When executing query:
      """
      MATCH (n:Sample)
      RETURN SIN(n.theta) AS s, COS(n.theta) AS c, TAN(n.theta) AS t
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — CARDINALITY(list): list cardinality.
  @covers:cardinalityExpression,cardinalityExpressionArgument,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [11] CARDINALITY of a list literal
    Given an empty graph
    When executing query:
      """
      RETURN CARDINALITY([1, 2, 3, 4]) AS n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — SIZE synonym for CARDINALITY.
  @covers:cardinalityExpression,cardinalityExpressionArgument,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [12] SIZE synonym for CARDINALITY
    Given an empty graph
    When executing query:
      """
      MATCH (n:Bag)
      RETURN SIZE(n.items) AS n
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — CHAR_LENGTH(s): character length.
  @covers:charLengthExpression,lengthExpression,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [13] CHAR_LENGTH of a string property
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN CHAR_LENGTH(n.name) AS len
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — CHARACTER_LENGTH spelled out.
  @covers:charLengthExpression,lengthExpression,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [14] CHARACTER_LENGTH spelled out
    Given an empty graph
    When executing query:
      """
      MATCH (n:Person)
      RETURN CHARACTER_LENGTH(n.name) AS len
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — OCTET_LENGTH / BYTE_LENGTH on a byte
  # string.  Both spellings are accepted as identifier function calls.
  @covers:byteLengthExpression,lengthExpression,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [15] OCTET_LENGTH byte length expression
    Given an empty graph
    When executing query:
      """
      MATCH (n:Blob)
      RETURN OCTET_LENGTH(n.payload) AS bytes
      """
    Then no side effects

  # ISO/IEC 39075:2024 §20.22 — PATH_LENGTH on a named path binding.
  @covers:pathLengthExpression,lengthExpression,numericValueFunction,valueFunction,valueExpression,returnStatement,returnStatementBody,returnItemList,returnItem,returnItemAlias
  Scenario: [16] PATH_LENGTH of a named path
    Given an empty graph
    When executing query:
      """
      MATCH p = (a:Person)-[:KNOWS]->(b:Person)
      RETURN PATH_LENGTH(p) AS hops
      """
    Then no side effects
