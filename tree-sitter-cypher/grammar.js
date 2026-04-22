/**
 * tree-sitter-cypher — Cypher / GQL grammar for the tree-sitter runtime.
 *
 * v1 scope (cy-h0p). Covers the cyrs v1 TCK surface plus additional
 * openCypher v9 / GQL-aligned constructs the cyrs Rust parser will accept:
 *
 *   - v0 baseline: MATCH / OPTIONAL MATCH / WHERE / WITH / RETURN /
 *     UNWIND / CREATE / MERGE / SET / REMOVE / DELETE.
 *   - UNION / UNION ALL at the top level (between SingleQuery tails).
 *   - CALL <procedure>(args) YIELD ... — the non-subquery form. The
 *     block-subquery form `CALL { ... }` is banned by spec §9.3 and
 *     falls out as an ERROR node because this rule requires `(` after
 *     the procedure name.
 *   - `shortestPath` / `allShortestPaths` patterns (spec §19).
 *   - Map projection: `n { .name, .age, *, key: expr }` (spec §19).
 *   - Pattern predicates: `EXISTS( (a)-->(b) )` (the call form). The
 *     block form `EXISTS { ... }` is banned by spec §9.3 and falls out
 *     as ERROR because the rule requires `(` after EXISTS.
 *   - List comprehension: `[x IN xs WHERE p | expr]` including the
 *     predicate-only and map-only forms.
 *   - List predicates: `ALL|ANY|NONE|SINGLE(x IN xs WHERE p)`.
 *   - List indexing / slicing: `xs[0]`, `xs[0..3]`, `xs[..3]`, `xs[1..]`.
 *
 * Rejections (§9.3): `CALL { ... }` block subquery, `EXISTS { ... }`
 * block subquery, `SHOW ...`, `CYPHER` prefixes, `LOAD CSV`, APOC
 * procedures. These all fall out naturally as ERROR nodes because
 * there is no grammar rule that accepts them — `SHOW` / `LOAD` / `CYPHER`
 * are not clause-start keywords; `CALL {` / `EXISTS {` do not match
 * because the call-shape rule demands `(`.
 *
 * The Rust CST in `cypher-syntax` remains authoritative — this grammar
 * is a parallel hand-maintained artefact kept in lock-step by the
 * `cargo xtask tree-sitter-parity` harness.
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
  // Map-projection "{...}" after a variable outranks map-literal parsing
  // inside return items so `RETURN n { .name }` projects `n` rather than
  // shift-reducing the trailing map as a separate return item.
  map_projection: 13,
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

  conflicts: ($) => [
    // `EXISTS((n) ...)` is ambiguous until we see either `)` (function
    // call of a variable inside a paren-expr) or `-` (pattern predicate).
    // Declare the conflict explicitly so tree-sitter generates a GLR
    // table rather than aborting; precedence picks `pattern_predicate`
    // when a relationship follows.
    [$.node_pattern, $.variable_expr],
    [$.node_pattern, $.map_projection_base],
    // `({})` is ambiguous: inside a paren-expr it's a map_literal; inside
    // a node_pattern it's a property_map. Both share the empty-map shape.
    [$.property_map, $.map_literal],
  ],

  rules: {
    // ===================================================================
    // Root — one or more statements, each a single-query with optional
    // UNION tails. Statements are `;`-separated.
    // ===================================================================
    source_file: ($) =>
      prec.right(
        seq(
          $._statement,
          repeat(seq(";", $._statement)),
          optional(";"),
        ),
      ),

    _statement: ($) =>
      prec.right(seq($._single_query, repeat($.union_tail))),

    _single_query: ($) => repeat1($._clause),

    /// `UNION` / `UNION ALL` followed by another single-query body.
    union_tail: ($) =>
      prec.right(
        seq(
          $.kw_union,
          optional($.kw_all),
          $._single_query,
        ),
      ),

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
        $.call_clause,
      ),

    // ===================================================================
    // Clauses
    // ===================================================================
    match_clause: ($) =>
      prec.right(seq($.kw_match, $.pattern, optional($.where_clause))),

    optional_match_clause: ($) =>
      prec.right(
        seq(
          $.kw_optional,
          $.kw_match,
          $.pattern,
          optional($.where_clause),
        ),
      ),

    where_clause: ($) => seq($.kw_where, $._expression),

    with_clause: ($) =>
      prec.right(
        seq(
          $.kw_with,
          optional($.kw_distinct),
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
          $.kw_return,
          optional($.kw_distinct),
          $._return_items,
          optional($.order_by),
          optional($.skip_subclause),
          optional($.limit_subclause),
        ),
      ),

    _return_items: ($) => commaSep1($.return_item),

    return_item: ($) =>
      seq($._expression, optional(seq($.kw_as, $.identifier))),

    order_by: ($) => seq($.kw_order, $.kw_by, commaSep1($.order_item)),

    order_item: ($) =>
      seq(
        $._expression,
        optional(
          choice(
            $.kw_ascending,
            $.kw_asc,
            $.kw_descending,
            $.kw_desc,
          ),
        ),
      ),

    skip_subclause: ($) => seq($.kw_skip, $._expression),
    limit_subclause: ($) => seq($.kw_limit, $._expression),

    unwind_clause: ($) =>
      seq($.kw_unwind, $._expression, $.kw_as, $.identifier),

    create_clause: ($) => seq($.kw_create, $.pattern),

    merge_clause: ($) =>
      prec.right(seq($.kw_merge, $.pattern, repeat($.merge_action))),

    merge_action: ($) =>
      seq(
        $.kw_on,
        choice($.kw_create, $.kw_match),
        $.set_clause,
      ),

    set_clause: ($) => seq($.kw_set, commaSep1($.set_item)),

    set_item: ($) =>
      choice(
        // Property assignment or variable replace: a.b.c = expr OR v = expr
        seq($._expression, "=", $._expression),
        // Label setting: v:Label[:Label]*
        seq($.identifier, $._label_list),
      ),

    remove_clause: ($) => seq($.kw_remove, commaSep1($.remove_item)),

    remove_item: ($) =>
      choice(
        $.property_access_expr,
        seq($.identifier, $._label_list),
      ),

    delete_clause: ($) =>
      seq(
        optional($.kw_detach),
        $.kw_delete,
        commaSep1($._expression),
      ),

    /// `CALL <procedure>(args) YIELD (items | *)` — non-subquery form.
    ///
    /// `CALL { ... }` block subqueries are banned by spec §9.3; this rule
    /// requires `(` after the procedure name so block forms fall through
    /// as ERROR nodes rather than being accepted. The procedure name
    /// itself is a dotted identifier path, e.g. `db.labels`,
    /// `org.neo4j.internal.myProc`.
    call_clause: ($) =>
      prec.right(
        seq(
          $.kw_call,
          $.procedure_name,
          "(",
          optional(commaSep1($._expression)),
          ")",
          optional($.yield_subclause),
        ),
      ),

    procedure_name: ($) =>
      seq($.identifier, repeat(seq(".", $.identifier))),

    yield_subclause: ($) =>
      seq(
        $.kw_yield,
        choice("*", commaSep1($.yield_item)),
      ),

    yield_item: ($) =>
      seq($.identifier, optional(seq($.kw_as, $.identifier))),

    // ===================================================================
    // Patterns
    // ===================================================================
    pattern: ($) => commaSep1($.pattern_part),

    pattern_part: ($) => choice($.named_pattern_part, $._anonymous_pattern),

    named_pattern_part: ($) =>
      seq(field("name", $.identifier), "=", $._anonymous_pattern),

    _anonymous_pattern: ($) =>
      choice(
        $.shortest_path_pattern,
        seq($.node_pattern, repeat(seq($.rel_pattern, $.node_pattern))),
      ),

    /// `shortestPath(pattern)` / `allShortestPaths(pattern)` — the pattern
    /// between the parentheses is a normal anonymous node/rel chain.
    shortest_path_pattern: ($) =>
      seq(
        choice($.kw_shortest_path, $.kw_all_shortest_paths),
        "(",
        $.node_pattern,
        repeat(seq($.rel_pattern, $.node_pattern)),
        ")",
      ),

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
        prec.left(PREC.or, seq($._expression, $.kw_or, $._expression)),
        prec.left(PREC.xor, seq($._expression, $.kw_xor, $._expression)),
        prec.left(PREC.and, seq($._expression, $.kw_and, $._expression)),
        prec.left(
          PREC.comparison,
          seq(
            $._expression,
            choice("=", "<>", "!=", "<", "<=", ">", ">=", $.kw_in),
            $._expression,
          ),
        ),
        prec.left(
          PREC.string_op,
          seq(
            $._expression,
            choice(
              seq($.kw_starts, $.kw_with),
              seq($.kw_ends, $.kw_with),
              $.kw_contains,
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
        prec(PREC.not, seq($.kw_not, $._expression)),
        prec(PREC.unary, seq("-", $._expression)),
        prec(PREC.unary, seq("+", $._expression)),
      ),

    _primary: ($) =>
      choice(
        $.literal_expr,
        $.list_comprehension,
        $.list_predicate,
        $.list_literal,
        $.map_projection,
        $.map_literal,
        $.parameter,
        $.case_expr,
        $.pattern_predicate,
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
          $.kw_is,
          optional($.kw_not),
          $.kw_null,
        ),
      ),

    paren_expr: ($) => seq("(", $._expression, ")"),

    function_call: ($) =>
      prec(
        PREC.primary,
        seq(
          field("name", choice($.identifier, $.kw_count, $.kw_exists)),
          "(",
          optional(seq(optional($.kw_distinct), commaSep1($._expression))),
          ")",
        ),
      ),

    variable_expr: ($) => $.identifier,

    /// `EXISTS( <anonymous-pattern> )` — the call-form pattern predicate
    /// from spec §19. The block form `EXISTS { ... }` is §9.3-banned and
    /// does NOT fit this rule because `{` is not the required opener.
    ///
    /// Precedence `primary + 1` prefers the pattern-predicate parse over
    /// the plain `EXISTS(expr)` function_call when the parenthesised
    /// content is a node pattern starting with `(`.
    pattern_predicate: ($) =>
      prec(
        PREC.primary + 1,
        seq(
          $.kw_exists,
          "(",
          $.node_pattern,
          repeat1(seq($.rel_pattern, $.node_pattern)),
          ")",
        ),
      ),

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

    /// `[x IN xs (WHERE pred)? (| map_expr)?]`
    ///
    /// Precedence above `list_literal` so the `x IN xs` prefix commits
    /// to the comprehension form before the list-literal rule could
    /// reduce `x` as a variable expression followed by `IN` as a
    /// comparison operator inside a list element.
    list_comprehension: ($) =>
      prec(
        PREC.primary + 1,
        seq(
          "[",
          $.identifier,
          $.kw_in,
          $._expression,
          optional(seq($.kw_where, $._expression)),
          optional(seq("|", $._expression)),
          "]",
        ),
      ),

    /// `ALL|ANY|NONE|SINGLE(x IN xs WHERE pred)` — parses as a call shape
    /// with a reserved predicate keyword as the function name. The
    /// interior is the same `x IN xs WHERE pred` binding as the list
    /// comprehension head (WHERE is required for list predicates).
    list_predicate: ($) =>
      prec(
        PREC.primary + 1,
        seq(
          field(
            "name",
            choice(
              $.kw_all,
              $.kw_any,
              $.kw_none,
              $.kw_single,
            ),
          ),
          "(",
          $.identifier,
          $.kw_in,
          $._expression,
          $.kw_where,
          $._expression,
          ")",
        ),
      ),

    map_literal: ($) =>
      seq(
        "{",
        optional(
          commaSep1(seq(choice($.identifier, $.string_literal), ":", $._expression)),
        ),
        "}",
      ),

    /// `<base> { .prop | .prop AS alias | key: expr | * }` —
    /// map projection (spec §19). Base is a variable or property access.
    ///
    /// Split into a dedicated `map_projection_base` rule so the
    /// conflict declaration at the top of this grammar can disambiguate
    /// between `variable_expr` and this wrapper; tree-sitter then
    /// prefers the projection parse when a `{` follows.
    map_projection: ($) =>
      prec.left(
        PREC.map_projection,
        seq(
          $.map_projection_base,
          "{",
          optional(commaSep1($.map_projection_item)),
          "}",
        ),
      ),

    map_projection_base: ($) => $.identifier,

    map_projection_item: ($) =>
      choice(
        // .prop — selector
        seq(".", $.identifier),
        // key: expr — literal entry
        seq($.identifier, ":", $._expression),
        // * — all properties passthrough
        "*",
      ),

    case_expr: ($) =>
      seq(
        $.kw_case,
        optional($._expression),
        repeat1($.case_when_arm),
        optional($.case_else_arm),
        $.kw_end,
      ),

    case_when_arm: ($) =>
      seq($.kw_when, $._expression, $.kw_then, $._expression),

    case_else_arm: ($) => seq($.kw_else, $._expression),

    // ===================================================================
    // Tokens
    // ===================================================================
    //
    // Keywords are promoted to named leaf nodes so highlight queries in
    // `queries/highlights.scm` can target them directly (tree-sitter
    // cannot address anonymous regex tokens by their source text). Every
    // keyword rule wraps `caseKw("...")` — the token-building helper at
    // the top of this file — so matching stays case-insensitive per
    // the Cypher spec.
    //
    // Bare-keyword rules below are referenced by clauses and expressions
    // instead of inlining `caseKw` at each use site.
    kw_match: ($) => caseKw("MATCH"),
    kw_optional: ($) => caseKw("OPTIONAL"),
    kw_where: ($) => caseKw("WHERE"),
    kw_with: ($) => caseKw("WITH"),
    kw_return: ($) => caseKw("RETURN"),
    kw_unwind: ($) => caseKw("UNWIND"),
    kw_create: ($) => caseKw("CREATE"),
    kw_merge: ($) => caseKw("MERGE"),
    kw_set: ($) => caseKw("SET"),
    kw_remove: ($) => caseKw("REMOVE"),
    kw_delete: ($) => caseKw("DELETE"),
    kw_detach: ($) => caseKw("DETACH"),
    kw_call: ($) => caseKw("CALL"),
    kw_yield: ($) => caseKw("YIELD"),
    kw_union: ($) => caseKw("UNION"),
    kw_all: ($) => caseKw("ALL"),
    kw_any: ($) => caseKw("ANY"),
    kw_none: ($) => caseKw("NONE"),
    kw_single: ($) => caseKw("SINGLE"),
    kw_on: ($) => caseKw("ON"),
    kw_as: ($) => caseKw("AS"),
    kw_distinct: ($) => caseKw("DISTINCT"),
    kw_order: ($) => caseKw("ORDER"),
    kw_by: ($) => caseKw("BY"),
    kw_skip: ($) => caseKw("SKIP"),
    kw_limit: ($) => caseKw("LIMIT"),
    kw_asc: ($) => caseKw("ASC"),
    kw_ascending: ($) => caseKw("ASCENDING"),
    kw_desc: ($) => caseKw("DESC"),
    kw_descending: ($) => caseKw("DESCENDING"),
    kw_case: ($) => caseKw("CASE"),
    kw_when: ($) => caseKw("WHEN"),
    kw_then: ($) => caseKw("THEN"),
    kw_else: ($) => caseKw("ELSE"),
    kw_end: ($) => caseKw("END"),
    kw_is: ($) => caseKw("IS"),
    kw_not: ($) => caseKw("NOT"),
    kw_null: ($) => caseKw("NULL"),
    kw_and: ($) => caseKw("AND"),
    kw_or: ($) => caseKw("OR"),
    kw_xor: ($) => caseKw("XOR"),
    kw_in: ($) => caseKw("IN"),
    kw_contains: ($) => caseKw("CONTAINS"),
    kw_starts: ($) => caseKw("STARTS"),
    kw_ends: ($) => caseKw("ENDS"),
    kw_count: ($) => caseKw("COUNT"),
    kw_exists: ($) => caseKw("EXISTS"),
    kw_shortest_path: ($) => caseKw("shortestPath"),
    kw_all_shortest_paths: ($) => caseKw("allShortestPaths"),
    kw_true: ($) => caseKw("TRUE"),
    kw_false: ($) => caseKw("FALSE"),

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

    bool_literal: ($) => choice($.kw_true, $.kw_false),

    null_literal: ($) => $.kw_null,

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
