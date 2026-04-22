; tree-sitter-cypher — scope-aware variable tracking (locals.scm).
;
; Editors that use locals queries (Neovim, Helix) will link every
; `@local.reference` to the nearest enclosing `@local.definition` inside
; the containing `@local.scope`. This gives variable-aware highlighting
; without teaching the grammar about binding semantics.
;
; Scope model:
;   - Each top-level statement (source_file) is a scope.
;   - A list comprehension `[x IN xs ...]` opens its own scope; `x` is a
;     definition local to that expression.
;   - A list predicate `ALL(x IN xs WHERE p)` is the same shape.
;   - Named path `p = (...)`: `p` is a definition in the enclosing
;     statement scope.
;
; Everything else (nodes, rel-binders, UNWIND binders, return aliases,
; YIELD aliases) defines into the statement scope.

; ---------------------------------------------------------------------------
; Scopes
; ---------------------------------------------------------------------------

(source_file)         @local.scope
(list_comprehension)  @local.scope
(list_predicate)      @local.scope

; ---------------------------------------------------------------------------
; Definitions
; ---------------------------------------------------------------------------

; Node-pattern binder: first identifier child of a node_pattern.
(node_pattern
  . (identifier) @local.definition)

; Rel-pattern binder: first identifier child of rel_detail.
(rel_detail
  . (identifier) @local.definition)

; Named path binder: `p = (...)`.
(named_pattern_part
  name: (identifier) @local.definition)

; UNWIND `<expr> AS <binder>` — the identifier after the AS keyword.
(unwind_clause
  (kw_as) . (identifier) @local.definition)

; Return aliases: `<expr> AS <alias>`.
(return_item
  (kw_as) . (identifier) @local.definition)

; List-comprehension / list-predicate binder: first identifier child.
(list_comprehension
  . (identifier) @local.definition)
(list_predicate
  . (identifier) @local.definition)

; YIELD item: `<name>` or `<name> AS <alias>` — the outer identifier
; binds the downstream reference.
(yield_item
  (identifier) @local.definition)

; ---------------------------------------------------------------------------
; References
; ---------------------------------------------------------------------------

(variable_expr
  (identifier) @local.reference)

(map_projection_base
  (identifier) @local.reference)

; Property access base is a reference; the trailing property name is not.
(property_access_expr
  (variable_expr
    (identifier) @local.reference))
