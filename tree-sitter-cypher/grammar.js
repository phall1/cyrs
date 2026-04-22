/**
 * tree-sitter-cypher — Cypher / GQL grammar for the tree-sitter runtime.
 *
 * v0 scope matches the cyrs TCK v1 surface (see
 * `crates/cypher-tck/tck/v1.toml`). This grammar is a parallel
 * hand-maintained artefact — the Rust CST is authoritative. The parity
 * harness `cargo xtask tree-sitter-parity` keeps the two in lock-step.
 *
 * Notes:
 * - Keywords are matched case-insensitively via the `caseKw` helper. Cypher
 *   keywords are not reserved at the identifier level per the GQL spec, but
 *   at the TCK v1 surface they behave reservedly — this grammar treats them
 *   as reserved to keep the LL(1) dispatch unambiguous.
 * - Operator precedence climbs roughly OR < AND < NOT < comparison <
 *   string-op < addition < multiplication < power < unary < postfix.
 */

const PREC = {
  or: 1,
  xor: 2,
  and: 3,
  not: 4,
  comparison: 5,
  string_op: 6,
  add: 7,
  mul: 8,
  pow: 9,
  unary: 10,
  postfix: 11,
  primary: 12,
};

/** Case-insensitive keyword matcher. */
function caseKw(word) {
  const chars = word
    .split("")
    .map((c) =>
      /[a-zA-Z]/.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c,
    )
    .join("");
  return new RegExp(chars);
}

/** Helper: comma-separated list of `rule`, at least one element. */
function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}

module.exports = grammar({
  name: "cypher",

  extras: ($) => [/\s+/, $.line_comment, $.block_comment],

  word: ($) => $.identifier,

  conflicts: ($) => [],

  rules: {
    // ===================================================================
    // Root
    // ===================================================================
    source_file: ($) => seq(repeat1($._clause), optional(";")),

    _clause: ($) =>
      choice(
        $.match_clause,
        $.optional_match_clause,
        $.with_clause,
        $.return_clause,
        $.unwind_clause,
        $.create_clause,
        $.merge_clause,
        $.set_clause,
        $.remove_clause,
        $.delete_clause,
      ),

    // ===================================================================
    // Clauses
    // ===================================================================
    match_clause: ($) =>
      prec.right(seq(caseKw("MATCH"), $.pattern, optional($.where_clause))),

    optional_match_clause: ($) =>
      prec.right(
        seq(
          caseKw("OPTIONAL"),
          caseKw("MATCH"),
          $.pattern,
          optional($.where_clause),
        ),
      ),

    where_clause: ($) => seq(caseKw("WHERE"), $._expression),

    with_clause: ($) =>
      prec.right(
        seq(
          caseKw("WITH"),
          optional(caseKw("DISTINCT")),
          $._return_items,
          optional($.order_by),
          optional($.skip_subclause),
          optional($.limit_subclause),
          optional($.where_clause),
        ),
      ),

    return_clause: ($) =>
      prec.right(
        seq(
          caseKw("RETURN"),
          optional(caseKw("DISTINCT")),
          $._return_items,
          optional($.order_by),
          optional($.skip_subclause),
          optional($.limit_subclause),
        ),
      ),

    _return_items: ($) => commaSep1($.return_item),

    return_item: ($) =>
      seq($._expression, optional(seq(caseKw("AS"), $.identifier))),

    order_by: ($) => seq(caseKw("ORDER"), caseKw("BY"), commaSep1($.order_item)),

    order_item: ($) =>
      seq(
        $._expression,
        optional(
          choice(
            caseKw("ASCENDING"),
            caseKw("ASC"),
            caseKw("DESCENDING"),
            caseKw("DESC"),
          ),
        ),
      ),

    skip_subclause: ($) => seq(caseKw("SKIP"), $._expression),
    limit_subclause: ($) => seq(caseKw("LIMIT"), $._expression),

    unwind_clause: ($) =>
      seq(caseKw("UNWIND"), $._expression, caseKw("AS"), $.identifier),

    create_clause: ($) => seq(caseKw("CREATE"), $.pattern),

    merge_clause: ($) =>
      prec.right(seq(caseKw("MERGE"), $.pattern, repeat($.merge_action))),

    merge_action: ($) =>
      seq(
        caseKw("ON"),
        choice(caseKw("CREATE"), caseKw("MATCH")),
        $.set_clause,
      ),

    set_clause: ($) => seq(caseKw("SET"), commaSep1($.set_item)),

    set_item: ($) =>
      choice(
        // Property assignment or variable replace: a.b.c = expr OR v = expr
        seq($._expression, "=", $._expression),
        // Label setting: v:Label[:Label]*
        seq($.identifier, $._label_list),
      ),

    remove_clause: ($) => seq(caseKw("REMOVE"), commaSep1($.remove_item)),

    remove_item: ($) =>
      choice(
        $.property_access_expr,
        seq($.identifier, $._label_list),
      ),

    delete_clause: ($) =>
      seq(
        optional(caseKw("DETACH")),
        caseKw("DELETE"),
        commaSep1($._expression),
      ),

    // ===================================================================
    // Patterns
    // ===================================================================
    pattern: ($) => commaSep1($.pattern_part),

    pattern_part: ($) => choice($.named_pattern_part, $._anonymous_pattern),

    named_pattern_part: ($) =>
      seq(field("name", $.identifier), "=", $._anonymous_pattern),

    _anonymous_pattern: ($) =>
      seq($.node_pattern, repeat(seq($.rel_pattern, $.node_pattern))),

    node_pattern: ($) =>
      seq(
        "(",
        optional($.identifier),
        optional($._label_list),
        optional($.property_map),
        ")",
      ),

    _label_list: ($) => repeat1(seq(":", $.identifier)),

    rel_pattern: ($) =>
      choice(
        // -[detail]->
        seq("-", optional($.rel_detail), "-", ">"),
        // <-[detail]-
        seq("<", "-", optional($.rel_detail), "-"),
        // -[detail]-
        seq("-", optional($.rel_detail), "-"),
      ),

    rel_detail: ($) =>
      seq(
        "[",
        optional($.identifier),
        optional($._rel_type_expr),
        optional($.rel_length),
        optional($.property_map),
        "]",
      ),

    _rel_type_expr: ($) =>
      seq(
        ":",
        $.identifier,
        repeat(seq("|", optional(":"), $.identifier)),
      ),

    rel_length: ($) =>
      seq(
        "*",
        optional(
          choice(
            seq($.int_literal, "..", optional($.int_literal)),
            seq("..", optional($.int_literal)),
            $.int_literal,
          ),
        ),
      ),

    property_map: ($) =>
      seq(
        "{",
        optional(commaSep1(seq($.identifier, ":", $._expression))),
        "}",
      ),

    // ===================================================================
    // Expressions
    // ===================================================================
    _expression: ($) =>
      choice(
        $.binary_expr,
        $.unary_expr,
        $._primary,
      ),

    binary_expr: ($) =>
      choice(
        prec.left(PREC.or, seq($._expression, caseKw("OR"), $._expression)),
        prec.left(PREC.xor, seq($._expression, caseKw("XOR"), $._expression)),
        prec.left(PREC.and, seq($._expression, caseKw("AND"), $._expression)),
        prec.left(
          PREC.comparison,
          seq(
            $._expression,
            choice("=", "<>", "!=", "<", "<=", ">", ">=", caseKw("IN")),
            $._expression,
          ),
        ),
        prec.left(
          PREC.string_op,
          seq(
            $._expression,
            choice(
              seq(caseKw("STARTS"), caseKw("WITH")),
              seq(caseKw("ENDS"), caseKw("WITH")),
              caseKw("CONTAINS"),
            ),
            $._expression,
          ),
        ),
        prec.left(PREC.add, seq($._expression, choice("+", "-"), $._expression)),
        prec.left(
          PREC.mul,
          seq($._expression, choice("*", "/", "%"), $._expression),
        ),
        prec.right(PREC.pow, seq($._expression, "^", $._expression)),
      ),

    unary_expr: ($) =>
      choice(
        prec(PREC.not, seq(caseKw("NOT"), $._expression)),
        prec(PREC.unary, seq("-", $._expression)),
        prec(PREC.unary, seq("+", $._expression)),
      ),

    _primary: ($) =>
      choice(
        $.literal_expr,
        $.list_literal,
        $.map_literal,
        $.parameter,
        $.case_expr,
        $.function_call,
        $.paren_expr,
        $._postfix,
        $.variable_expr,
      ),

    _postfix: ($) =>
      choice(
        $.property_access_expr,
        $.subscript_expr,
        $.is_null_expr,
      ),

    property_access_expr: ($) =>
      prec.left(PREC.postfix, seq($._primary, ".", $.identifier)),

    subscript_expr: ($) =>
      prec.left(
        PREC.postfix,
        seq(
          $._primary,
          "[",
          choice(
            // slice [i..j] / [..j] / [i..] / [..]
            seq(optional($._expression), "..", optional($._expression)),
            $._expression,
          ),
          "]",
        ),
      ),

    is_null_expr: ($) =>
      prec.left(
        PREC.postfix,
        seq(
          $._primary,
          caseKw("IS"),
          optional(caseKw("NOT")),
          caseKw("NULL"),
        ),
      ),

    paren_expr: ($) => seq("(", $._expression, ")"),

    function_call: ($) =>
      prec(
        PREC.primary,
        seq(
          field("name", choice($.identifier, caseKw("COUNT"), caseKw("EXISTS"))),
          "(",
          optional(seq(optional(caseKw("DISTINCT")), commaSep1($._expression))),
          ")",
        ),
      ),

    variable_expr: ($) => $.identifier,

    literal_expr: ($) =>
      choice(
        $.int_literal,
        $.float_literal,
        $.string_literal,
        $.bool_literal,
        $.null_literal,
      ),

    list_literal: ($) =>
      seq("[", optional(commaSep1($._expression)), "]"),

    map_literal: ($) =>
      seq(
        "{",
        optional(
          commaSep1(seq(choice($.identifier, $.string_literal), ":", $._expression)),
        ),
        "}",
      ),

    case_expr: ($) =>
      seq(
        caseKw("CASE"),
        optional($._expression),
        repeat1($.case_when_arm),
        optional($.case_else_arm),
        caseKw("END"),
      ),

    case_when_arm: ($) =>
      seq(caseKw("WHEN"), $._expression, caseKw("THEN"), $._expression),

    case_else_arm: ($) => seq(caseKw("ELSE"), $._expression),

    // ===================================================================
    // Tokens
    // ===================================================================
    parameter: ($) => seq("$", choice($.identifier, $.int_literal)),

    int_literal: ($) => /\d+/,
    float_literal: ($) =>
      token(choice(/\d+\.\d+([eE][+-]?\d+)?/, /\d+[eE][+-]?\d+/)),

    string_literal: ($) =>
      token(
        choice(
          seq("'", repeat(choice(/[^'\\]/, /\\./)), "'"),
          seq('"', repeat(choice(/[^"\\]/, /\\./)), '"'),
        ),
      ),

    bool_literal: ($) => choice(caseKw("TRUE"), caseKw("FALSE")),

    null_literal: ($) => caseKw("NULL"),

    // Identifier: regular `[A-Za-z_][A-Za-z0-9_]*` OR backtick-escaped.
    // The `word` rule points at this, so tree-sitter uses it for keyword
    // extraction.
    identifier: ($) =>
      token(
        choice(
          /[A-Za-z_][A-Za-z0-9_]*/,
          seq("`", repeat(choice(/[^`]/, "``")), "`"),
        ),
      ),

    line_comment: ($) => token(seq("//", /[^\n]*/)),

    block_comment: ($) => token(seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")),
  },
});
