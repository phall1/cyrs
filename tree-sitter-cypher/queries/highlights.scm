; tree-sitter-cypher — semantic-tokens-grade highlighting for Cypher / GQL.
;
; Targets Neovim (nvim-treesitter), Helix, and Zed out of the box. Capture
; names follow the `nvim-treesitter` canonical capture set
; (https://github.com/nvim-treesitter/nvim-treesitter/blob/main/CONTRIBUTING.md#highlights)
; which Helix and Zed both understand.
;
; Coverage claim:
;   - Every clause keyword (MATCH / OPTIONAL / WHERE / WITH / RETURN /
;     UNWIND / CREATE / MERGE / SET / REMOVE / DELETE / DETACH / CALL /
;     YIELD / UNION / ON / AS / DISTINCT / ORDER / BY / SKIP / LIMIT /
;     ASC / DESC / ASCENDING / DESCENDING / CASE / WHEN / THEN / ELSE /
;     END / IS / NOT / NULL / AND / OR / XOR / IN / CONTAINS / STARTS /
;     ENDS / COUNT / EXISTS / ALL / ANY / NONE / SINGLE / shortestPath /
;     allShortestPaths / TRUE / FALSE).
;   - All literals (int / float / string / bool / null).
;   - Identifiers in binding positions (node-pattern variable, rel-pattern
;     variable, UNWIND binder, return alias, list-comprehension / list-
;     predicate binder, YIELD alias, CALL procedure name).
;   - Labels (node and rel-type) via the `:` prefix context.
;   - Properties via `.` postfix context.
;   - Operators, parentheses, brackets, braces, and arrow-like
;     punctuation.
;   - Comments.
;
; Keywords are named leaf nodes (kw_match, kw_return, ...) in the grammar
; so we can target them directly without literal-string matching (which
; tree-sitter can't do for case-insensitive regex tokens).

; ---------------------------------------------------------------------------
; Keywords — clause heads
; ---------------------------------------------------------------------------

[
  (kw_match)
  (kw_optional)
  (kw_where)
  (kw_with)
  (kw_return)
  (kw_unwind)
  (kw_create)
  (kw_merge)
  (kw_set)
  (kw_remove)
  (kw_delete)
  (kw_detach)
  (kw_call)
  (kw_yield)
  (kw_union)
  (kw_on)
  (kw_order)
  (kw_by)
  (kw_skip)
  (kw_limit)
  (kw_as)
  (kw_distinct)
  (kw_asc)
  (kw_ascending)
  (kw_desc)
  (kw_descending)
] @keyword

[
  (kw_case)
  (kw_when)
  (kw_then)
  (kw_else)
  (kw_end)
] @keyword.conditional

; Logical / comparison operator keywords
[
  (kw_and)
  (kw_or)
  (kw_xor)
  (kw_not)
  (kw_in)
  (kw_contains)
  (kw_starts)
  (kw_ends)
  (kw_is)
] @keyword.operator

; List-predicate / pattern-predicate / shortest-path heads → builtin calls
[
  (kw_all)
  (kw_any)
  (kw_none)
  (kw_single)
  (kw_count)
  (kw_exists)
  (kw_shortest_path)
  (kw_all_shortest_paths)
] @function.builtin

; Null / boolean literals
(null_literal) @constant.builtin
(bool_literal) @boolean

; Common openCypher stdlib functions get a builtin colour too.
(function_call
  name: (identifier) @function.builtin
  (#match? @function.builtin "^(?i)(count|exists|size|id|head|tail|last|type|labels|keys|nodes|relationships|length|coalesce|toInteger|toFloat|toString|toBoolean|reduce)$"))

; ---------------------------------------------------------------------------
; Literals
; ---------------------------------------------------------------------------

(int_literal)    @number
(float_literal)  @number.float
(string_literal) @string

; ---------------------------------------------------------------------------
; Identifiers — bindings first, then references
; ---------------------------------------------------------------------------

; UNWIND binder: the trailing identifier after AS.
(unwind_clause
  (kw_as) . (identifier) @variable)

; Return aliases: `<expr> AS <alias>`.
(return_item
  (kw_as) . (identifier) @variable)

; YIELD items: bare name or `name AS alias`.
(yield_item
  (identifier) @variable)

; Node-pattern binder: first identifier inside node_pattern.
(node_pattern . (identifier) @variable)

; Rel-pattern binder: first identifier inside rel_detail.
(rel_detail . (identifier) @variable)

; Named path binder: `p = ...`.
(named_pattern_part
  name: (identifier) @variable)

; List-comprehension / list-predicate binder.
(list_comprehension
  . (identifier) @variable)
(list_predicate
  . (identifier) @variable)

; Parameters: `$name` / `$0`.
(parameter) @variable.parameter

; ---------------------------------------------------------------------------
; Labels and types (always follow `:`)
; ---------------------------------------------------------------------------

; A label on a node pattern is any non-binder identifier inside the
; pattern. We rely on the `_label_list` rule shape: `:` `identifier`
; repeated. Capture the identifier; colour by uppercase-first to avoid
; colouring lowercase keyword-looking names.
((node_pattern
  (_)
  (identifier) @type)
 (#match? @type "^[A-Z]"))

((rel_detail
  (identifier) @type)
 (#match? @type "^[A-Z]"))

; ---------------------------------------------------------------------------
; Properties (after `.`)
; ---------------------------------------------------------------------------

(property_access_expr
  "." (identifier) @property)

(map_projection_item
  "." (identifier) @property)

; Map literal keys.
(map_literal
  (identifier) @property)

; Property-map keys inside node/rel patterns.
(property_map
  (identifier) @property)

; Map-projection literal entry key: `key: expr` — first identifier child.
(map_projection_item
  . (identifier) @property)

; ---------------------------------------------------------------------------
; Procedure name — dotted identifier, coloured as function call target
; ---------------------------------------------------------------------------

(procedure_name
  (identifier) @function)

(function_call
  name: (identifier) @function)

; ---------------------------------------------------------------------------
; Catch-all variable references
; ---------------------------------------------------------------------------

(variable_expr
  (identifier) @variable)

(map_projection_base
  (identifier) @variable)

; ---------------------------------------------------------------------------
; Operators, punctuation, comments
; ---------------------------------------------------------------------------

[
  "="
  "<>"
  "!="
  "<"
  "<="
  ">"
  ">="
  "+"
  "-"
  "*"
  "/"
  "%"
  "^"
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
  ":"
  "."
  "|"
  ".."
  "$"
] @punctuation.delimiter

(line_comment)  @comment
(block_comment) @comment
