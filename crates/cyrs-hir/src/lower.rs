//! AST → HIR lowering (spec 0001 §6.1).
//!
//! Entry point: [`lower_statement`]. One lowering pass per Cypher statement.
//!
//! # Scope
//!
//! The lowerer covers all syntactic constructs that the cy-nom parser
//! currently emits (MATCH, OPTIONAL MATCH, WHERE, RETURN) plus every
//! expression form in the current grammar. `VarId`s are allocated for
//! bound variables as the pattern is walked; use-sites are lowered to
//! [`Expr::Unresolved`] — full name resolution is deferred to the
//! `cy-nres` bead.
//!
//! # Deferred constructs
//!
//! The following are not yet parsed by the cy-nom grammar and so cannot
//! be lowered here. When those beads land, add arms to the relevant
//! `lower_*` helpers:
//!
//! - WITH / UNWIND / CREATE / MERGE / SET / REMOVE / DELETE / CALL clauses
//! - Variable-length relationships (`*m..n`)
//! - Path binders (`p = (a)-[]->(b)`)
//! - List comprehensions, pattern comprehensions, CASE expressions,
//!   pattern predicates in expression position, EXISTS, COUNT(*) standalone

use indexmap::IndexMap;
use smol_str::SmolStr;

use cyrs_syntax::{Parse, SyntaxElement, SyntaxKind, SyntaxNode, TextRange, parse};

use crate::{
    BinOp, Binding, Clause, Direction, Expr, HirId, ListPredKind, MapProjectionItem, OrderItem,
    Pattern, PatternElement, PatternPart, Projection, RelLength, RemoveItem, SetItem, ShortestPath,
    Statement, UnaryOp, VarId, VarKind,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Lower a single-statement Cypher source into an HIR [`Statement`].
///
/// The `src` string must contain exactly one statement. If the parser
/// produces multiple `STATEMENT` nodes, only the first is lowered.
///
/// Every lowered HIR node's [`HirId`] is recorded in
/// [`Statement::node_map`] pointing back at its originating
/// [`SyntaxNode`], satisfying the AST↔HIR map requirement (spec §6.1).
///
/// This is a thin wrapper around [`lower_parse`] that runs the parser
/// first. Callers that already hold a [`Parse`] (e.g. an embedder that
/// also needs the syntax errors) should call [`lower_parse`] directly to
/// avoid re-running the lexer and parser — see also [`crate::parse_to_hir`].
pub fn lower_statement(src: &str) -> Statement {
    lower_parse(&parse(src))
}

/// Lower an already-computed [`Parse`] into an HIR [`Statement`].
///
/// This is the primitive that does the AST→HIR walk; [`lower_statement`]
/// is a sugar wrapper that calls [`cyrs_syntax::parse`] first. Embedders
/// that already need the [`Parse`] (e.g. to surface
/// [`cyrs_syntax::SyntaxError`]s) should call this directly so the
/// lexer + parser pipeline is not run twice.
///
/// The `Parse` is borrowed; only the underlying `SyntaxNode` is walked,
/// so callers retain ownership.
pub fn lower_parse(parse: &Parse) -> Statement {
    let root = parse.syntax();
    // --- cy-lp3y SESSION SET HIR ---
    // GQL `SESSION SET …` is a top-level statement category that sits
    // directly under `SOURCE_FILE` — never wrapped in a `STATEMENT`
    // node (see `cyrs_syntax::grammar` mod.rs). Prefer it when present
    // before falling through to the query-statement path so a file like
    // `SESSION SET GRAPH g` lowers to a `SessionSetHir`-bearing
    // [`Statement`] instead of an empty clause list.
    if let Some(session_node) = root
        .children()
        .find(|n| n.kind() == SyntaxKind::SESSION_SET_STMT)
    {
        let span = session_node.text_range();
        let mut ctx = LowerCtx {
            stmt: Statement::new(span),
            var_names: IndexMap::new(),
            next_var: 0,
        };
        ctx.lower_session_set_stmt(session_node);
        return ctx.stmt;
    }
    // --- end cy-lp3y ---

    // Find the first STATEMENT child of SOURCE_FILE.
    let stmt_node = root
        .children()
        .find(|n| n.kind() == SyntaxKind::STATEMENT)
        .unwrap_or_else(|| root.clone());

    let span = stmt_node.text_range();
    let mut ctx = LowerCtx {
        stmt: Statement::new(span),
        var_names: IndexMap::new(),
        next_var: 0,
    };
    ctx.lower_stmt_node(stmt_node);
    ctx.stmt
}

// ---------------------------------------------------------------------------
// Lowering context
// ---------------------------------------------------------------------------

struct LowerCtx {
    stmt: Statement,
    /// Map from variable name to its `VarId` — populated on first binding.
    var_names: IndexMap<SmolStr, VarId>,
    /// Monotonic counter for allocating `VarId`s.
    next_var: u32,
}

impl LowerCtx {
    // --- id / var allocation -----------------------------------------------

    fn alloc_hir(&mut self, node: SyntaxNode) -> HirId {
        self.stmt.alloc_id(node)
    }

    /// Intern or create a [`VarId`] for `name`, recording it in the bindings
    /// table with the given kind and definition span.
    fn bind_var(&mut self, name: &str, kind: VarKind, defined_at: TextRange) -> VarId {
        let key: SmolStr = SmolStr::new(name);
        if let Some(&id) = self.var_names.get(&key) {
            return id;
        }
        let id = VarId(self.next_var);
        self.next_var += 1;
        self.var_names.insert(key.clone(), id);
        self.stmt.bindings.insert(
            id,
            Binding {
                id,
                name: key,
                kind,
                defined_at,
            },
        );
        id
    }

    /// Resolve a variable name to its `VarId` if already bound, otherwise
    /// return `None` (caller wraps in `Expr::Unresolved`).
    fn resolve_var(&self, name: &str) -> Option<VarId> {
        self.var_names.get(name).copied()
    }

    // --- statement / clause -------------------------------------------------

    fn lower_stmt_node(&mut self, node: SyntaxNode) {
        for child in node.children() {
            self.lower_clause_into(child);
        }
    }

    // --- cy-lp3y SESSION SET HIR ---
    /// Lower a `SESSION_SET_STMT` node into [`Statement::session_set`].
    ///
    /// Mirrors the discriminant in [`cyrs_ast::SessionSetVariant`].
    /// Recovery shapes (malformed inputs the parser still produced an
    /// outer node for) lower to whichever sub-variant survives the AST
    /// cast; if none does, the field is left `None` and downstream
    /// passes treat the statement as a no-op.
    fn lower_session_set_stmt(&mut self, node: SyntaxNode) {
        use cyrs_ast::{SessionSetStmt, SessionSetVariant};

        let span = node.text_range();
        let id = self.alloc_hir(node.clone());

        let Some(stmt) = SessionSetStmt::cast(node.clone()) else {
            // Recovery: outer node existed but the AST cast failed.
            // Leave session_set unset; sema's syntax-error path covers
            // the user-visible diagnostic.
            return;
        };

        let Some(variant) = stmt.variant() else {
            // Outer node present but no variant child survived recovery.
            return;
        };

        let variant_hir = match variant {
            SessionSetVariant::Graph(g) => {
                let v_span = g.syntax().text_range();
                let is_property_graph = g.is_property_graph();
                let graph_ref = g
                    .graph_ref_token()
                    .map(|t| SmolStr::new(t.text()))
                    .unwrap_or_default();
                crate::SessionSetVariantHir::Graph {
                    graph_ref,
                    is_property_graph,
                    span: v_span,
                }
            }
            SessionSetVariant::TimeZone(tz) => {
                let v_span = tz.syntax().text_range();
                let time_zone = tz
                    .time_zone_token()
                    .map(|t| SmolStr::new(t.text()))
                    .unwrap_or_default();
                crate::SessionSetVariantHir::TimeZone {
                    time_zone,
                    span: v_span,
                }
            }
            SessionSetVariant::Value(val) => {
                let v_span = val.syntax().text_range();
                let if_not_exists = val.is_if_not_exists();
                let param = val
                    .param_token()
                    .map(|t| SmolStr::new(t.text()))
                    .unwrap_or_default();
                // RHS expression. The AST `value_expr()` accessor uses
                // the typed `Expr` cast which doesn't recognise every
                // raw expression production (`LITERAL_EXPR`, `VAR_EXPR`,
                // `PARAM_EXPR`, …); walk the variant's child nodes
                // directly and pick the first one `try_lower_expr`
                // accepts, mirroring how WITH / UNWIND trailer
                // expressions are picked up downstream.
                let value = val
                    .syntax()
                    .children()
                    .find_map(|c| self.try_lower_expr(c))
                    .unwrap_or(Expr::Null);
                crate::SessionSetVariantHir::Value {
                    param,
                    if_not_exists,
                    value,
                    span: v_span,
                }
            }
        };

        self.stmt.session_set = Some(crate::SessionSetHir {
            id,
            variant: variant_hir,
            span,
        });
    }
    // --- end cy-lp3y ---

    /// Lower a clause subtree and push the resulting HIR clause(s) onto
    /// [`Statement::clauses`] **in source order**.
    ///
    /// Invariant (cy-ypm, spec §6.1): the order of clauses in
    /// `stmt.clauses` MUST mirror the surface-syntax order.  In
    /// particular, an inline `WHERE` that lives inside a
    /// `MATCH (p) WHERE q` node lowers to two sibling HIR clauses
    /// `[Match, Where]` — the `Match` is pushed BEFORE the `Where`, even
    /// though it is built last.  Downstream plan lowering relies on this
    /// (`Filter` must chain after the `Source` the owning `MATCH`
    /// produced); violating it silently orphans predicates onto
    /// `EMPTY_SOURCE`.
    fn lower_clause_into(&mut self, node: SyntaxNode) {
        let span = node.text_range();
        match node.kind() {
            SyntaxKind::MATCH_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let pattern = self.lower_pattern_from_clause(&node);
                // Build the inline-WHERE first so it shares byte-adjacent
                // HirIds, but push MATCH first to preserve source order.
                let inline_where = self.build_inline_where(&node);
                self.stmt.clauses.push(Clause::Match {
                    id,
                    optional: false,
                    pattern,
                    span,
                });
                if let Some(w) = inline_where {
                    self.stmt.clauses.push(w);
                }
            }
            SyntaxKind::OPTIONAL_MATCH_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let pattern = self.lower_pattern_from_clause(&node);
                let inline_where = self.build_inline_where(&node);
                self.stmt.clauses.push(Clause::Match {
                    id,
                    optional: true,
                    pattern,
                    span,
                });
                if let Some(w) = inline_where {
                    self.stmt.clauses.push(w);
                }
            }
            _ => {
                if let Some(clause) = self.try_lower_clause(node) {
                    self.stmt.clauses.push(clause);
                }
            }
        }
    }

    /// Lower the inline `WHERE_CLAUSE` child of a `MATCH_CLAUSE` /
    /// `OPTIONAL_MATCH_CLAUSE` into a standalone [`Clause::Where`], if
    /// present.  Caller is responsible for pushing it onto
    /// `stmt.clauses` **after** the owning MATCH (see
    /// [`Self::lower_clause_into`] for the ordering invariant).
    fn build_inline_where(&mut self, match_node: &SyntaxNode) -> Option<Clause> {
        let where_node = match_node
            .children()
            .find(|n| n.kind() == SyntaxKind::WHERE_CLAUSE)?;
        let wspan = where_node.text_range();
        let wid = self.alloc_hir(where_node.clone());
        let pred = where_node
            .children()
            .find_map(|n| self.try_lower_expr(n))
            .unwrap_or(Expr::Null);
        Some(Clause::Where {
            id: wid,
            predicate: pred,
            span: wspan,
        })
    }

    /// Lower a single syntax node into an HIR [`Clause`], for clause
    /// kinds that always produce exactly one HIR clause.  `MATCH` /
    /// `OPTIONAL_MATCH` go through [`Self::lower_clause_into`] instead
    /// because they may emit a trailing inline-`WHERE` clause.
    fn try_lower_clause(&mut self, node: SyntaxNode) -> Option<Clause> {
        let span = node.text_range();
        match node.kind() {
            SyntaxKind::MATCH_CLAUSE | SyntaxKind::OPTIONAL_MATCH_CLAUSE => {
                // Callers should use `lower_clause_into` for MATCH /
                // OPTIONAL_MATCH to preserve source-order for the inline
                // WHERE. See the invariant documented there.
                unreachable!(
                    "MATCH / OPTIONAL_MATCH must be lowered via lower_clause_into to preserve \
                     clause ordering (cy-ypm, spec §6.1)"
                );
            }
            SyntaxKind::WHERE_CLAUSE => {
                // Standalone WHERE (between clauses).
                let id = self.alloc_hir(node.clone());
                let pred = node
                    .children()
                    .find_map(|n| self.try_lower_expr(n))
                    .unwrap_or(Expr::Null);
                Some(Clause::Where {
                    id,
                    predicate: pred,
                    span,
                })
            }
            SyntaxKind::RETURN_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let distinct = has_token(&node, SyntaxKind::DISTINCT_KW);
                let projections = self.lower_return_body(&node);
                let (order_by, skip, limit) = self.lower_return_trailers(&node);
                Some(Clause::Return {
                    id,
                    projections,
                    distinct,
                    order_by,
                    skip,
                    limit,
                    span,
                })
            }
            SyntaxKind::UNWIND_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let list = node
                    .children()
                    .find_map(|n| self.try_lower_expr(n))
                    .unwrap_or(Expr::Null);
                // Binder is the trailing NAME child.
                let bind = node
                    .children()
                    .find(|n| n.kind() == SyntaxKind::NAME)
                    .map_or(VarId(u32::MAX), |name_node| {
                        let name = ident_text(&name_node).unwrap_or_default();
                        self.bind_var(&name, VarKind::Value, name_node.text_range())
                    });
                Some(Clause::Unwind {
                    id,
                    list,
                    bind,
                    span,
                })
            }
            SyntaxKind::CREATE_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let pattern = self.lower_pattern_from_clause(&node);
                Some(Clause::Create { id, pattern, span })
            }
            SyntaxKind::MERGE_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let pattern = self.lower_pattern_from_clause(&node);
                // Walk MERGE_ACTION children: each contains `ON CREATE SET …`
                // or `ON MATCH SET …`. Discriminate by the presence of a
                // CREATE_KW / MATCH_KW token child.
                let mut on_create = Vec::new();
                let mut on_match = Vec::new();
                for action in node
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::MERGE_ACTION)
                {
                    let items = action
                        .children()
                        .find(|n| n.kind() == SyntaxKind::SET_CLAUSE)
                        .map(|s| self.lower_set_items(&s))
                        .unwrap_or_default();
                    if has_token(&action, SyntaxKind::CREATE_KW) {
                        on_create.extend(items);
                    } else {
                        on_match.extend(items);
                    }
                }
                Some(Clause::Merge {
                    id,
                    pattern,
                    on_create,
                    on_match,
                    span,
                })
            }
            SyntaxKind::SET_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let items = self.lower_set_items(&node);
                Some(Clause::Set { id, items, span })
            }
            SyntaxKind::REMOVE_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let items = self.lower_remove_items(&node);
                Some(Clause::Remove { id, items, span })
            }
            SyntaxKind::DELETE_CLAUSE => {
                let id = self.alloc_hir(node.clone());
                let detach = has_token(&node, SyntaxKind::DETACH_KW);
                let targets = node
                    .children()
                    .filter_map(|n| self.try_lower_expr(n))
                    .collect();
                Some(Clause::Delete {
                    id,
                    targets,
                    detach,
                    span,
                })
            }
            SyntaxKind::WITH_CLAUSE => {
                // WITH shares the same RETURN_BODY shape so reuse the helper;
                // its optional inline WHERE becomes the projection's `filter`
                // predicate (HIR `Clause::With { filter }`), not a separate
                // `Where` clause — §6.4 models it as part of the same frame.
                // The parser nests WHERE_CLAUSE inside RETURN_BODY, so search
                // descendants rather than direct children (cy-v31).
                let id = self.alloc_hir(node.clone());
                let projections = self.lower_return_body(&node);
                let filter = node
                    .descendants()
                    .find(|n| n.kind() == SyntaxKind::WHERE_CLAUSE)
                    .and_then(|w| w.children().find_map(|n| self.try_lower_expr(n)));
                let (order_by, skip, limit) = self.lower_return_trailers(&node);
                Some(Clause::With {
                    id,
                    projections,
                    filter,
                    order_by,
                    skip,
                    limit,
                    span,
                })
            }
            // All other clause types are stubs in the cy-nom parser and
            // produce ERROR nodes — skip them silently.
            _ => None,
        }
    }

    // --- pattern ------------------------------------------------------------

    fn lower_pattern_from_clause(&mut self, clause_node: &SyntaxNode) -> Pattern {
        // Collect all PATTERN children of the clause. A PATTERN now has
        // either a PATTERN_PART child (plain) or a NAMED_PATTERN_PART
        // (path binder `p = ...`). For named parts, the binder identifier
        // lives inside a NAME child before the inner PATTERN_PART.
        let parts: Vec<PatternPart> = clause_node
            .children()
            .filter(|n| n.kind() == SyntaxKind::PATTERN)
            .flat_map(|pat| self.lower_pattern_wrapper(&pat))
            .collect();
        Pattern { parts }
    }

    /// Unwrap a `PATTERN` node into its `PatternPart`s, handling both the
    /// plain `PATTERN_PART` and the `NAMED_PATTERN_PART` (`p = PATTERN_PART`)
    /// shapes.
    fn lower_pattern_wrapper(&mut self, pat: &SyntaxNode) -> Vec<PatternPart> {
        let mut out = Vec::new();
        for child in pat.children() {
            match child.kind() {
                SyntaxKind::PATTERN_PART => {
                    out.push(self.lower_pattern_part(child));
                }
                // cy-eaq: `shortestPath((a)-[*]->(b))` and
                // `allShortestPaths(...)` wrap an inner PATTERN_PART. We
                // lower the inner pattern and record the shortest-path
                // discriminant on the `PatternPart` so the plan layer
                // can surface a dedicated `ReadOp::ShortestPath` rather
                // than a plain var-length `Expand`. The path-binder case
                // (`p = ...`) is handled below via NAMED_PATTERN_PART,
                // which can wrap either a plain PATTERN_PART or a
                // SHORTEST_PATH_PATTERN.
                SyntaxKind::SHORTEST_PATH_PATTERN => {
                    if let Some(inner) = child
                        .children()
                        .find(|n| n.kind() == SyntaxKind::PATTERN_PART)
                    {
                        let mut part = self.lower_pattern_part(inner);
                        part.shortest = shortest_path_kind(&child);
                        out.push(part);
                    }
                }
                SyntaxKind::NAMED_PATTERN_PART => {
                    // The binder name is the NAME child's IDENT.
                    let bind =
                        child
                            .children()
                            .find(|n| n.kind() == SyntaxKind::NAME)
                            .map(|name_node| {
                                let name = ident_text(&name_node).unwrap_or_default();
                                self.bind_var(&name, VarKind::Path, name_node.text_range())
                            });
                    // The inner shape is either PATTERN_PART (plain) or
                    // SHORTEST_PATH_PATTERN (wrapping its own
                    // PATTERN_PART). The shortest-path discriminant is
                    // recorded on the resulting `PatternPart` so the
                    // plan layer can lower `p = shortestPath(...)` to a
                    // dedicated `ReadOp::ShortestPath`.
                    let inner_part = child.children().find_map(|n| match n.kind() {
                        SyntaxKind::PATTERN_PART => Some((n, ShortestPath::No)),
                        SyntaxKind::SHORTEST_PATH_PATTERN => {
                            let kind = shortest_path_kind(&n);
                            n.children()
                                .find(|c| c.kind() == SyntaxKind::PATTERN_PART)
                                .map(|c| (c, kind))
                        }
                        _ => None,
                    });
                    if let Some((inner, kind)) = inner_part {
                        let mut part = self.lower_pattern_part(inner);
                        part.named_as = bind;
                        part.shortest = kind;
                        out.push(part);
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn lower_pattern_part(&mut self, node: SyntaxNode) -> PatternPart {
        let elements = self.lower_pattern_elements(&node);
        PatternPart {
            named_as: None,
            elements,
            shortest: crate::ShortestPath::No,
        }
    }

    /// Walk a `PATTERN_PART` node and collect interleaved `NODE_PATTERN`s and
    /// `REL_PATTERN`s as a flat `Vec<PatternElement>`.
    fn lower_pattern_elements(&mut self, part: &SyntaxNode) -> Vec<PatternElement> {
        let mut elems = Vec::new();
        for child in part.children() {
            match child.kind() {
                SyntaxKind::NODE_PATTERN => {
                    elems.push(self.lower_node_pattern(child));
                }
                SyntaxKind::REL_PATTERN => {
                    elems.push(self.lower_rel_pattern(child));
                }
                _ => {}
            }
        }
        elems
    }

    fn lower_node_pattern(&mut self, node: SyntaxNode) -> PatternElement {
        let span = node.text_range();
        let id = self.alloc_hir(node.clone());

        // Optional variable binding: the NAME child of NODE_PATTERN.
        let bind = node
            .children()
            .find(|n| n.kind() == SyntaxKind::NAME)
            .map(|name_node| {
                let name = ident_text(&name_node).unwrap_or_default();
                self.bind_var(&name, VarKind::Node, name_node.text_range())
            });

        // Labels: each COLON + IDENT pair inside LABEL_EXPR.
        let labels = node
            .children()
            .find(|n| n.kind() == SyntaxKind::LABEL_EXPR)
            .map(|le| collect_label_idents(&le))
            .unwrap_or_default();

        // Shorthand property map → lowered to Expr::Map.
        let props = node
            .children()
            .find(|n| n.kind() == SyntaxKind::PROPERTY_MAP)
            .map(|pm| self.lower_property_map(pm));

        PatternElement::Node {
            id,
            bind,
            labels,
            props,
            span,
        }
    }

    fn lower_rel_pattern(&mut self, node: SyntaxNode) -> PatternElement {
        let span = node.text_range();
        let id = self.alloc_hir(node.clone());

        // Direction: presence of ARROW_L token or ARROW_R token.
        let has_left = has_token(&node, SyntaxKind::ARROW_L);
        let has_right = has_token(&node, SyntaxKind::ARROW_R);
        let direction = match (has_left, has_right) {
            (true, false) => Direction::Incoming,
            (false, true) => Direction::Outgoing,
            _ => Direction::Undirected,
        };

        // REL_DETAIL child — variable, type, props.
        let detail = node.children().find(|n| n.kind() == SyntaxKind::REL_DETAIL);

        let bind = detail.as_ref().and_then(|d| {
            d.children()
                .find(|n| n.kind() == SyntaxKind::NAME)
                .map(|name_node| {
                    let name = ident_text(&name_node).unwrap_or_default();
                    self.bind_var(&name, VarKind::Relationship, name_node.text_range())
                })
        });

        let types = detail
            .as_ref()
            .and_then(|d| d.children().find(|n| n.kind() == SyntaxKind::REL_TYPE_EXPR))
            .map(|rte| {
                // REL_TYPE_EXPR contains COLON + IDENT.
                rte.children_with_tokens()
                    .filter_map(SyntaxElement::into_token)
                    .filter(|t| {
                        t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT
                    })
                    .map(|t| SmolStr::new(t.text()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let props = detail
            .as_ref()
            .and_then(|d| d.children().find(|n| n.kind() == SyntaxKind::PROPERTY_MAP))
            .map(|pm| self.lower_property_map(pm));

        // Variable-length hop quantifier `*m..n` / `*m` / `*..n` / `*`.
        // Detected by presence of a REL_LENGTH child inside REL_DETAIL;
        // extract bounds from its IntLiteral tokens and DOT_DOT presence.
        let length = detail
            .as_ref()
            .and_then(|d| d.children().find(|n| n.kind() == SyntaxKind::REL_LENGTH))
            .map_or(RelLength::Single, |rl| lower_rel_length(&rl));

        PatternElement::Rel {
            id,
            bind,
            types,
            direction,
            length,
            props,
            span,
        }
    }

    // --- write-clause items -------------------------------------------------

    /// Lower a `SET_CLAUSE` node's `SET_ITEM` children into HIR `SetItem`s.
    /// Each item is one of the four shapes in the ungrammar:
    /// `prop_access '=' expr`, `target LabelExpr`, `target '=' expr`,
    /// `target '+=' expr`. We keep the first two (property assign + label
    /// add) as the v1 surface; the `target = / +=` whole-replace forms are
    /// lowered to `SetItem::AssignMap` with `replace = true/false`.
    fn lower_set_items(&mut self, clause: &SyntaxNode) -> Vec<SetItem> {
        let mut out = Vec::new();
        for item in clause
            .children()
            .filter(|n| n.kind() == SyntaxKind::SET_ITEM)
        {
            if let Some(it) = self.try_lower_set_item(&item) {
                out.push(it);
            }
        }
        out
    }

    fn try_lower_set_item(&mut self, item: &SyntaxNode) -> Option<SetItem> {
        // Label-add: `target:Lab` — recognise by the LABEL_EXPR child.
        if let Some(labels_node) = item.children().find(|n| n.kind() == SyntaxKind::LABEL_EXPR) {
            let labels = collect_label_idents(&labels_node);
            let target_name = item
                .children()
                .find(|n| n.kind() == SyntaxKind::VAR_EXPR)
                .and_then(|v| ident_text(&v))
                .unwrap_or_default();
            let target_var = self
                .resolve_var(&target_name)
                .unwrap_or_else(|| self.bind_var(&target_name, VarKind::Node, item.text_range()));
            return Some(SetItem::Labels {
                target: target_var,
                labels,
            });
        }
        // Property assign: `a.prop = expr` — PROP_ACCESS_EXPR + one trailing Expr.
        if let Some(prop_node) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::PROP_ACCESS_EXPR)
        {
            let target = self.try_lower_expr(prop_node.clone()).unwrap_or(Expr::Null);
            let prop = prop_node
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                .map(|t| SmolStr::new(t.text()))
                .unwrap_or_default();
            let value = item
                .children()
                .filter_map(|n| self.try_lower_expr(n))
                .nth(1)
                .unwrap_or(Expr::Null);
            return Some(SetItem::Property {
                target,
                prop,
                value,
            });
        }
        // Whole-node replace / merge: `target = expr` / `target += expr`.
        if let Some(var_node) = item.children().find(|n| n.kind() == SyntaxKind::VAR_EXPR) {
            let name = ident_text(&var_node).unwrap_or_default();
            let target_var = self
                .resolve_var(&name)
                .unwrap_or_else(|| self.bind_var(&name, VarKind::Node, var_node.text_range()));
            let map = item
                .children()
                .filter_map(|n| self.try_lower_expr(n))
                .nth(1)
                .unwrap_or(Expr::Null);
            return Some(SetItem::AssignMap {
                target: target_var,
                map,
                replace: true,
            });
        }
        None
    }

    fn lower_remove_items(&mut self, clause: &SyntaxNode) -> Vec<RemoveItem> {
        let mut out = Vec::new();
        for item in clause
            .children()
            .filter(|n| n.kind() == SyntaxKind::REMOVE_ITEM)
        {
            if let Some(labels_node) = item.children().find(|n| n.kind() == SyntaxKind::LABEL_EXPR)
            {
                let labels = collect_label_idents(&labels_node);
                let name = item
                    .children()
                    .find(|n| n.kind() == SyntaxKind::VAR_EXPR)
                    .and_then(|v| ident_text(&v))
                    .unwrap_or_default();
                let target = self
                    .resolve_var(&name)
                    .unwrap_or_else(|| self.bind_var(&name, VarKind::Node, item.text_range()));
                out.push(RemoveItem::Labels { target, labels });
                continue;
            }
            if let Some(prop_node) = item
                .children()
                .find(|n| n.kind() == SyntaxKind::PROP_ACCESS_EXPR)
            {
                let target = self.try_lower_expr(prop_node.clone()).unwrap_or(Expr::Null);
                let prop = prop_node
                    .children_with_tokens()
                    .filter_map(SyntaxElement::into_token)
                    .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                    .map(|t| SmolStr::new(t.text()))
                    .unwrap_or_default();
                out.push(RemoveItem::Property { target, prop });
            }
        }
        out
    }

    // --- return body --------------------------------------------------------

    fn lower_return_body(&mut self, clause: &SyntaxNode) -> Vec<Projection> {
        // RETURN_CLAUSE → RETURN_BODY → RETURN_ITEMS → RETURN_ITEM*
        clause
            .children()
            .find(|n| n.kind() == SyntaxKind::RETURN_BODY)
            .and_then(|body| {
                body.children()
                    .find(|n| n.kind() == SyntaxKind::RETURN_ITEMS)
            })
            .map(|items| {
                items
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::RETURN_ITEM)
                    .map(|ri| self.lower_return_item(ri))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn lower_return_item(&mut self, node: SyntaxNode) -> Projection {
        let span = node.text_range();

        // RETURN_ITEM contains: Expr? AS_KW? IDENT?
        // The STAR case (`RETURN *`) has no Expr child — we represent it
        // as `Expr::Unresolved("*")` since there's no HIR Expr::Star yet.
        let has_star = has_token(&node, SyntaxKind::STAR);
        let expr = if has_star {
            Expr::Unresolved(SmolStr::new("*"))
        } else {
            node.children()
                .find_map(|n| self.try_lower_expr(n))
                .unwrap_or(Expr::Null)
        };

        // Alias: AS_KW followed by IDENT token — both are direct token
        // children of RETURN_ITEM.
        let alias = if has_token(&node, SyntaxKind::AS_KW) {
            node.children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                .map(|t| SmolStr::new(t.text()))
        } else {
            None
        };

        Projection { expr, alias, span }
    }

    /// Lower the `ORDER BY` / `SKIP` / `LIMIT` trailer subclauses of a
    /// RETURN / WITH clause (spec §6.1 trailer).
    ///
    /// The parser nests these as children of the clause's `RETURN_BODY`
    /// (for RETURN) or directly under the WITH clause node — both shapes
    /// are matched by walking `descendants()` here. Each trailer is
    /// optional; absent trailers yield empty vec / `None`.
    fn lower_return_trailers(
        &mut self,
        clause: &SyntaxNode,
    ) -> (Vec<OrderItem>, Option<Expr>, Option<Expr>) {
        let mut order_by = Vec::new();
        let mut skip = None;
        let mut limit = None;

        // Trailers live inside RETURN_BODY (for RETURN_CLAUSE) and as
        // descendants for WITH_CLAUSE (same nesting). Iterate descendants
        // and dispatch by kind.
        for n in clause.descendants() {
            match n.kind() {
                SyntaxKind::ORDER_BY if order_by.is_empty() => {
                    for item in n.children().filter(|c| c.kind() == SyntaxKind::ORDER_ITEM) {
                        order_by.push(self.lower_order_item(item));
                    }
                }
                SyntaxKind::SKIP_SUBCLAUSE if skip.is_none() => {
                    skip = n.children().find_map(|c| self.try_lower_expr(c));
                }
                SyntaxKind::LIMIT_SUBCLAUSE if limit.is_none() => {
                    limit = n.children().find_map(|c| self.try_lower_expr(c));
                }
                _ => {}
            }
        }

        (order_by, skip, limit)
    }

    fn lower_order_item(&mut self, node: SyntaxNode) -> OrderItem {
        let span = node.text_range();
        let expr = node
            .children()
            .find_map(|c| self.try_lower_expr(c))
            .unwrap_or(Expr::Null);
        // Direction: DESC / DESCENDING ⇒ descending; ASC / ASCENDING /
        // absent ⇒ ascending. Tokens live directly on ORDER_ITEM.
        let descending =
            has_token(&node, SyntaxKind::DESC_KW) || has_token(&node, SyntaxKind::DESCENDING_KW);
        OrderItem {
            expr,
            descending,
            span,
        }
    }

    // --- expressions --------------------------------------------------------

    /// Try to lower a node as an expression. Returns `None` for node kinds
    /// that are not expressions (so callers can filter with `find_map`).
    fn try_lower_expr(&mut self, node: SyntaxNode) -> Option<Expr> {
        match node.kind() {
            SyntaxKind::LITERAL_EXPR => Some(lower_literal(node)),
            SyntaxKind::PARAM_EXPR => Some(lower_param(node)),
            SyntaxKind::VAR_EXPR => Some(self.lower_var_expr(node)),
            SyntaxKind::PROP_ACCESS_EXPR => Some(self.lower_prop_access(node)),
            SyntaxKind::SUBSCRIPT_EXPR => Some(self.lower_subscript(node)),
            SyntaxKind::INDEX_EXPR => Some(self.lower_index_expr(node)),
            SyntaxKind::SLICE_EXPR => Some(self.lower_slice_expr(node)),
            SyntaxKind::FUNCTION_CALL => Some(self.lower_function_call(node)),
            SyntaxKind::PAREN_EXPR => {
                // Unwrap the inner expression.
                node.children()
                    .find_map(|n| self.try_lower_expr(n))
                    .or(Some(Expr::Null))
            }
            SyntaxKind::BINARY_EXPR => Some(self.lower_binary_expr(node)),
            SyntaxKind::UNARY_EXPR => Some(self.lower_unary_expr(node)),
            SyntaxKind::IS_NULL_EXPR => Some(self.lower_is_null_expr(node)),
            SyntaxKind::IN_EXPR => Some(self.lower_in_expr(node)),
            SyntaxKind::REGEX_MATCH_EXPR => Some(self.lower_regex_match_expr(node)),
            SyntaxKind::STRING_OP_EXPR => Some(self.lower_string_op_expr(node)),
            SyntaxKind::LIST_LITERAL => Some(self.lower_list_literal(node)),
            SyntaxKind::MAP_LITERAL => Some(self.lower_property_map(node)),
            SyntaxKind::CASE_EXPR => Some(self.lower_case_expr(node)),
            SyntaxKind::LIST_COMPREHENSION => Some(self.lower_list_comprehension(node)),
            SyntaxKind::LIST_PREDICATE_EXPR => Some(self.lower_list_predicate(node)),
            SyntaxKind::MAP_PROJECTION => Some(self.lower_map_projection(node)),
            SyntaxKind::PATTERN_PREDICATE => Some(self.lower_pattern_predicate(node)),
            // --- cy-p1u5 EXISTS parser-only ---
            // `EXISTS { … }` / `EXISTS ( MATCH … )` (ISO §10.7 / §14.10,
            // bead cy-p1u5). Lower to an inert `ExistsSubqueryDeferred`
            // marker — the body is **not** descended into, no `VarId`s
            // are minted for it, and name resolution does not enter
            // the inner clauses. The sema dialect-gate pass (cyrs-sema
            // `dialect::check_dialect`) emits `DiagCode::E4017`
            // (`exists_subquery` gate) on every occurrence; spec §20
            // D1 / N4 remain in force at the semantic surface.
            SyntaxKind::EXISTS_SUBQUERY_EXPR => Some(Expr::ExistsSubqueryDeferred {
                span: node.text_range(),
            }),
            // --- end cy-p1u5 ---
            _ => None,
        }
    }

    fn lower_var_expr(&mut self, node: SyntaxNode) -> Expr {
        let name = ident_text(&node).unwrap_or_default();
        self.resolve_var(&name)
            .map_or_else(|| Expr::Unresolved(SmolStr::new(name)), Expr::Var)
    }

    fn lower_prop_access(&mut self, node: SyntaxNode) -> Expr {
        // PROP_ACCESS_EXPR: first child Expr, then DOT, then IDENT.
        let target = node
            .children()
            .find_map(|n| self.try_lower_expr(n))
            .unwrap_or(Expr::Null);
        let prop = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
            .map(|t| SmolStr::new(t.text()))
            .unwrap_or_default();
        Expr::Prop {
            target: Box::new(target),
            prop,
        }
    }

    fn lower_subscript(&mut self, node: SyntaxNode) -> Expr {
        let mut children = node.children().filter_map(|n| self.try_lower_expr(n));
        let target = children.next().unwrap_or(Expr::Null);
        let index = children.next().unwrap_or(Expr::Null);
        Expr::Index {
            target: Box::new(target),
            index: Box::new(index),
        }
    }

    /// Lower `INDEX_EXPR` (`xs[i]`) — identical shape to `SUBSCRIPT_EXPR`
    /// but produced by the typed cy-7s6.1 grammar path so the HIR sees a
    /// distinct node and the sema `kinds` pass can apply list-only
    /// semantics (openCypher negative indices, out-of-bounds → null).
    ///
    /// v1 scope: LIST only. STRING indexing is deferred (see scope note
    /// on `Expr::Slice` / `cyrs-sema::kinds` for the E5010 emit site).
    fn lower_index_expr(&mut self, node: SyntaxNode) -> Expr {
        let mut children = node.children().filter_map(|n| self.try_lower_expr(n));
        let target = children.next().unwrap_or(Expr::Null);
        let index = children.next().unwrap_or(Expr::Null);
        Expr::Index {
            target: Box::new(target),
            index: Box::new(index),
        }
    }

    /// Lower `SLICE_EXPR` (`xs[i..j]`, `xs[..j]`, `xs[i..]`).
    ///
    /// The `DOT_DOT` token separates start from end inside the bracket.
    /// Either bound may be elided: child expressions appearing before the
    /// `..` token are the start; those appearing after are the end. The
    /// first child is always the slice target.
    ///
    /// Negative indices and out-of-bounds values are valid openCypher
    /// shapes and carry their semantics at evaluation time — the HIR
    /// does not normalise them.
    fn lower_slice_expr(&mut self, node: SyntaxNode) -> Expr {
        // Walk children-with-tokens to preserve source ordering of both
        // nodes and the `..` separator. A SLICE_EXPR has, in source order:
        //   <target Expr> '[' <start Expr?> '..' <end Expr?> ']'
        // Target is always present (receiver of the postfix); start and
        // end are optional.
        let mut target: Option<Expr> = None;
        let mut start: Option<Expr> = None;
        let mut end: Option<Expr> = None;
        let mut past_lbrack = false;
        let mut past_dotdot = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(tok) => match tok.kind() {
                    SyntaxKind::L_BRACK => past_lbrack = true,
                    SyntaxKind::DOT_DOT => past_dotdot = true,
                    _ => {}
                },
                SyntaxElement::Node(n) => {
                    if let Some(expr) = self.try_lower_expr(n) {
                        if !past_lbrack {
                            target = Some(expr);
                        } else if !past_dotdot {
                            start = Some(expr);
                        } else {
                            end = Some(expr);
                        }
                    }
                }
            }
        }

        Expr::Slice {
            target: Box::new(target.unwrap_or(Expr::Null)),
            start: start.map(Box::new),
            end: end.map(Box::new),
        }
    }

    fn lower_function_call(&mut self, node: SyntaxNode) -> Expr {
        // FUNCTION_CALL: lhs VAR_EXPR holds the name, ARG_LIST holds args.
        // `count` is a reserved keyword (COUNT_KW) so it doesn't tokenise as
        // IDENT — treat it as a function name when it appears in head
        // position. Other aggregates (sum/avg/min/max/collect) are plain
        // identifiers and round-trip via IDENT.
        let name = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| {
                matches!(
                    t.kind(),
                    SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT | SyntaxKind::COUNT_KW
                )
            })
            .map(|t| SmolStr::new(t.text().to_ascii_lowercase()))
            .or_else(|| {
                node.children()
                    .find(|n| n.kind() == SyntaxKind::VAR_EXPR)
                    .and_then(|v| ident_text(&v))
                    .map(SmolStr::new)
            });

        let distinct = has_token(&node, SyntaxKind::DISTINCT_KW);

        let args = node
            .children()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
            .map(|al| {
                al.children()
                    .filter_map(|n| self.try_lower_expr(n))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Expr::Call {
            name: name.unwrap_or_else(|| SmolStr::new("")),
            args,
            distinct,
        }
    }

    fn lower_binary_expr(&mut self, node: SyntaxNode) -> Expr {
        // BINARY_EXPR: child[0] lhs, op token, child[1] rhs.
        let mut exprs = node.children().filter_map(|n| self.try_lower_expr(n));
        let lhs = exprs.next().unwrap_or(Expr::Null);
        let rhs = exprs.next().unwrap_or(Expr::Null);

        // Find the operator token (non-trivia token that is not a node kind).
        let op_tok = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| !t.kind().is_trivia() && !t.kind().is_node());

        let op = op_tok
            .as_ref()
            .and_then(|t| bin_op_from_kind(t.kind()))
            .unwrap_or(BinOp::Add);

        Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    fn lower_unary_expr(&mut self, node: SyntaxNode) -> Expr {
        let op_tok = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| !t.kind().is_trivia());
        let op = op_tok.as_ref().map_or(UnaryOp::Neg, |t| match t.kind() {
            SyntaxKind::NOT_KW => UnaryOp::Not,
            _ => UnaryOp::Neg,
        });
        let operand = node
            .children()
            .find_map(|n| self.try_lower_expr(n))
            .unwrap_or(Expr::Null);
        Expr::UnaryOp {
            op,
            operand: Box::new(operand),
        }
    }

    fn lower_is_null_expr(&mut self, node: SyntaxNode) -> Expr {
        let negated = has_token(&node, SyntaxKind::NOT_KW);
        let operand = node
            .children()
            .find_map(|n| self.try_lower_expr(n))
            .unwrap_or(Expr::Null);
        Expr::IsNull {
            operand: Box::new(operand),
            negated,
        }
    }

    fn lower_in_expr(&mut self, node: SyntaxNode) -> Expr {
        let mut exprs = node.children().filter_map(|n| self.try_lower_expr(n));
        let operand = exprs.next().unwrap_or(Expr::Null);
        let list = exprs.next().unwrap_or(Expr::Null);
        Expr::InList {
            operand: Box::new(operand),
            list: Box::new(list),
        }
    }

    fn lower_regex_match_expr(&mut self, node: SyntaxNode) -> Expr {
        let mut exprs = node.children().filter_map(|n| self.try_lower_expr(n));
        let lhs = exprs.next().unwrap_or(Expr::Null);
        let rhs = exprs.next().unwrap_or(Expr::Null);
        Expr::BinOp {
            op: BinOp::RegexMatch,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    fn lower_string_op_expr(&mut self, node: SyntaxNode) -> Expr {
        // STRING_OP_EXPR: STARTS WITH, ENDS WITH, CONTAINS.
        let op = if has_token(&node, SyntaxKind::STARTS_KW) {
            BinOp::StartsWith
        } else if has_token(&node, SyntaxKind::ENDS_KW) {
            BinOp::EndsWith
        } else {
            BinOp::Contains
        };
        let mut exprs = node.children().filter_map(|n| self.try_lower_expr(n));
        let lhs = exprs.next().unwrap_or(Expr::Null);
        let rhs = exprs.next().unwrap_or(Expr::Null);
        Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    fn lower_list_literal(&mut self, node: SyntaxNode) -> Expr {
        let items = node
            .children()
            .filter_map(|n| self.try_lower_expr(n))
            .collect();
        Expr::List(items)
    }

    /// Lower a `PROPERTY_MAP` node into `Expr::Map`.
    fn lower_property_map(&mut self, node: SyntaxNode) -> Expr {
        // Tokens and children are interleaved. Strategy:
        // scan children_with_tokens for IDENT (key) followed by Expr child.
        let mut pairs: Vec<(SmolStr, Expr)> = Vec::new();
        let mut pending_key: Option<SmolStr> = None;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT {
                        pending_key = Some(SmolStr::new(t.text()));
                    }
                    // COLON / COMMA / braces are skipped.
                }
                SyntaxElement::Node(n) => {
                    if let Some(expr) = self.try_lower_expr(n)
                        && let Some(key) = pending_key.take()
                    {
                        pairs.push((key, expr));
                    }
                }
            }
        }
        Expr::Map(pairs)
    }

    fn lower_case_expr(&mut self, node: SyntaxNode) -> Expr {
        // CASE_EXPR: optional scrutinee Expr (direct child, before any arms),
        //            CASE_WHEN_ARM+,
        //            CASE_ELSE_ARM? (optional).
        // Children appear in syntactic order. The scrutinee is *any* direct
        // Expr child that precedes the first `CASE_WHEN_ARM`; generic CASE
        // forms omit it (the first direct child is a `CASE_WHEN_ARM`).
        // cy-41u, spec §19 row "CASE".
        let mut scrutinee: Option<Box<Expr>> = None;
        let mut arms: Vec<(Expr, Expr)> = Vec::new();
        let mut otherwise: Option<Box<Expr>> = None;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::CASE_WHEN_ARM => {
                    let mut arm_exprs = child.children().filter_map(|n| self.try_lower_expr(n));
                    let when_value = arm_exprs.next().unwrap_or(Expr::Null);
                    let then_value = arm_exprs.next().unwrap_or(Expr::Null);
                    arms.push((when_value, then_value));
                }
                SyntaxKind::CASE_ELSE_ARM => {
                    let else_value = child
                        .children()
                        .find_map(|n| self.try_lower_expr(n))
                        .unwrap_or(Expr::Null);
                    otherwise = Some(Box::new(else_value));
                }
                _ => {
                    // Direct Expr child: at most one is legal, and it
                    // always precedes the first arm. If arms have already
                    // been collected we treat stray Expr children as
                    // recovery noise and skip.
                    if arms.is_empty()
                        && scrutinee.is_none()
                        && let Some(expr) = self.try_lower_expr(child)
                    {
                        scrutinee = Some(Box::new(expr));
                    }
                }
            }
        }

        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        }
    }

    /// Lower a `LIST_COMPREHENSION` node into [`Expr::ListComprehension`].
    ///
    /// Expected AST shape (spec §6.1):
    /// ```text
    /// LIST_COMPREHENSION
    ///   NAME          — the iteration variable (x in `[x IN xs WHERE p | e]`)
    ///   IN_KW
    ///   <iterable>    — expression for the source list (xs)
    ///   WHERE_KW?
    ///   <filter>?     — optional predicate expression (p)
    ///   PIPE?
    ///   <map_expr>?   — optional projection expression (e)
    /// ```
    fn lower_list_comprehension(&mut self, node: SyntaxNode) -> Expr {
        // Iteration variable — NAME child.
        let var_name = node
            .children()
            .find(|n| n.kind() == SyntaxKind::NAME)
            .and_then(|n| ident_text(&n))
            .unwrap_or_else(|| "_".to_string());

        // Allocate a VarId for the iteration variable.
        let filter_var = self.bind_var(&var_name, VarKind::Value, node.text_range());

        // Walk expression children in order to find iterable, filter, map_expr.
        // Children order after NAME: iterable, [filter], [map_expr].
        let expr_children: Vec<Expr> = node
            .children()
            .filter(|n| n.kind() != SyntaxKind::NAME)
            .filter_map(|n| self.try_lower_expr(n))
            .collect();

        let has_where = has_token(&node, SyntaxKind::WHERE_KW);
        let has_pipe = has_token(&node, SyntaxKind::PIPE);

        let iterable = expr_children.first().cloned().unwrap_or(Expr::Null);

        let (filter, map_expr) = match (has_where, has_pipe) {
            (true, true) => {
                // [x IN xs WHERE p | e] — 3 exprs: iterable, predicate, map
                let filter = expr_children.get(1).cloned();
                let map = expr_children
                    .get(2)
                    .cloned()
                    .unwrap_or(Expr::Var(filter_var));
                (filter.map(Box::new), Box::new(map))
            }
            (false, true) => {
                // [x IN xs | e] — 2 exprs: iterable, map
                let map = expr_children
                    .get(1)
                    .cloned()
                    .unwrap_or(Expr::Var(filter_var));
                (None, Box::new(map))
            }
            (true, false) => {
                // [x IN xs WHERE p] — filter only, map = identity
                let filter = expr_children.get(1).cloned();
                (filter.map(Box::new), Box::new(Expr::Var(filter_var)))
            }
            (false, false) => {
                // [x IN xs] — plain collect
                (None, Box::new(Expr::Var(filter_var)))
            }
        };

        Expr::ListComprehension {
            filter_var,
            iterable: Box::new(iterable),
            filter,
            map_expr,
        }
    }

    /// Lower a `LIST_PREDICATE_EXPR` node into [`Expr::ListPredicate`]
    /// (cy-8x5, spec §19 row "List predicates").
    ///
    /// Expected AST shape:
    /// ```text
    /// LIST_PREDICATE_EXPR
    ///   (ANY_KW | ALL_KW | NONE_KW | SINGLE_KW)   — discriminant
    ///   L_PAREN
    ///   NAME                                      — binder x
    ///   IN_KW
    ///   <iterable>                                — xs
    ///   (WHERE_KW <predicate>)?                   — optional p(x)
    ///   R_PAREN
    /// ```
    ///
    /// The binder is introduced as a fresh `Value` variable scoped to the
    /// predicate expression only (parallel to `ListComprehension`'s
    /// `filter_var` — callers descending into `predicate` must push a
    /// child scope first; see `scope.rs`).
    fn lower_list_predicate(&mut self, node: SyntaxNode) -> Expr {
        // Discriminant — first keyword token child.
        let kind = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find_map(|t| match t.kind() {
                SyntaxKind::ANY_KW => Some(ListPredKind::Any),
                SyntaxKind::ALL_KW => Some(ListPredKind::All),
                SyntaxKind::NONE_KW => Some(ListPredKind::None),
                SyntaxKind::SINGLE_KW => Some(ListPredKind::Single),
                _ => None,
            })
            .unwrap_or(ListPredKind::Any);

        // Binder name — NAME child.
        let var_name = node
            .children()
            .find(|n| n.kind() == SyntaxKind::NAME)
            .and_then(|n| ident_text(&n))
            .unwrap_or_else(|| "_".to_string());
        let var = self.bind_var(&var_name, VarKind::Value, node.text_range());

        // Iterable = first Expr child; predicate = second Expr child (present
        // iff WHERE_KW is in the token stream).
        let expr_children: Vec<Expr> = node
            .children()
            .filter(|n| n.kind() != SyntaxKind::NAME)
            .filter_map(|n| self.try_lower_expr(n))
            .collect();

        let iterable = expr_children.first().cloned().unwrap_or(Expr::Null);
        let predicate = if has_token(&node, SyntaxKind::WHERE_KW) {
            expr_children.get(1).cloned().map(Box::new)
        } else {
            None
        };

        Expr::ListPredicate {
            kind,
            var,
            iterable: Box::new(iterable),
            predicate,
        }
    }

    /// Lower a `MAP_PROJECTION` node into [`Expr::MapProjection`].
    ///
    /// Expected AST shape (spec §6.1):
    /// ```text
    /// MAP_PROJECTION
    ///   <base_expr>               — the base expression (e.g. variable `a`)
    ///   L_BRACE
    ///   MAP_PROJECTION_ITEM*
    ///     DOT IDENT               → PropCopy
    ///     IDENT COLON Expr        → Computed
    ///     IDENT                   → VarShorthand
    ///   R_BRACE
    /// ```
    fn lower_map_projection(&mut self, node: SyntaxNode) -> Expr {
        // First expression child is the base.
        let base = node
            .children()
            .find_map(|n| self.try_lower_expr(n))
            .unwrap_or(Expr::Null);

        let items = node
            .children()
            .filter(|n| n.kind() == SyntaxKind::MAP_PROJECTION_ITEM)
            .map(|item_node| self.lower_map_projection_item(item_node))
            .collect();

        Expr::MapProjection {
            base: Box::new(base),
            items,
        }
    }

    fn lower_map_projection_item(&mut self, node: SyntaxNode) -> MapProjectionItem {
        let has_dot = has_token(&node, SyntaxKind::DOT);
        let has_colon = has_token(&node, SyntaxKind::COLON);

        if has_dot {
            // `.prop` — property copy shorthand.
            let prop = node
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                .map(|t| SmolStr::new(t.text()))
                .unwrap_or_default();
            MapProjectionItem::PropCopy { prop }
        } else if has_colon {
            // `key: expr` — computed key-value pair.
            let key = node
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                .map(|t| SmolStr::new(t.text()))
                .unwrap_or_default();
            let value = node
                .children()
                .find_map(|n| self.try_lower_expr(n))
                .unwrap_or(Expr::Null);
            MapProjectionItem::Computed { key, value }
        } else {
            // `varName` — variable shorthand.
            let name = node
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                .map(|t| SmolStr::new(t.text()))
                .unwrap_or_default();
            let var = self
                .resolve_var(name.as_str())
                .unwrap_or_else(|| self.bind_var(name.as_str(), VarKind::Value, node.text_range()));
            MapProjectionItem::VarShorthand { var, name }
        }
    }

    /// Lower a `PATTERN_PREDICATE` node into [`Expr::PatternPredicate`].
    ///
    /// A pattern predicate in expression position — e.g. in
    /// `WHERE (a)-->(:X)` — is lowered to an existential check on the
    /// contained pattern. Downstream sema produces the actual existence
    /// check; HIR just flags it as `PatternPredicate`.
    fn lower_pattern_predicate(&mut self, node: SyntaxNode) -> Expr {
        // PATTERN_PREDICATE wraps one or more PATTERN children.
        let parts: Vec<PatternPart> = node
            .children()
            .filter(|n| n.kind() == SyntaxKind::PATTERN || n.kind() == SyntaxKind::PATTERN_PART)
            .flat_map(|pat| {
                if pat.kind() == SyntaxKind::PATTERN {
                    pat.children()
                        .filter(|n| n.kind() == SyntaxKind::PATTERN_PART)
                        .map(|pp| self.lower_pattern_part(pp))
                        .collect::<Vec<_>>()
                } else {
                    vec![self.lower_pattern_part(pat)]
                }
            })
            .collect();

        // If the parser emitted no inner PATTERN nodes (e.g. it's a stub),
        // attempt to parse nested NODE_PATTERN elements directly.
        let parts = if parts.is_empty() {
            let elems = self.lower_pattern_elements(&node);
            if elems.is_empty() {
                vec![]
            } else {
                vec![PatternPart {
                    named_as: None,
                    shortest: crate::ShortestPath::No,
                    elements: elems,
                }]
            }
        } else {
            parts
        };

        Expr::PatternPredicate(Pattern { parts })
    }
}

// ---------------------------------------------------------------------------
// Token-level helpers (free functions — no `self` needed)
// ---------------------------------------------------------------------------

/// Returns true if `node` has a direct token child of `kind`.
fn has_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .any(|t| t.kind() == kind)
}

/// Classify a `SHORTEST_PATH_PATTERN` node by its leading keyword token.
///
/// The parser keeps the `shortestPath` / `allShortestPaths` keyword as a
/// child token of the node (see `grammar::pattern::shortest_path_pattern`),
/// so the discriminant is recoverable here. A node without either keyword
/// (a recovery shape) degrades to [`ShortestPath::Shortest`].
fn shortest_path_kind(node: &SyntaxNode) -> ShortestPath {
    if has_token(node, SyntaxKind::ALLSHORTESTPATHS_KW) {
        ShortestPath::AllShortest
    } else {
        ShortestPath::Shortest
    }
}

/// Extract the text of the first `IDENT` or `QUOTED_IDENT` token that is a
/// direct token descendant of `node`, or inside a `NAME` child.
fn ident_text(node: &SyntaxNode) -> Option<String> {
    // Walk tokens shallowly (direct children_with_tokens). `COUNT_KW` is a
    // reserved keyword token but can stand in for an identifier in expression
    // position (`count(n)`); the parser wraps it in a `VAR_EXPR` so it shows
    // up here when we're chasing a function name. Lowercase so the rest of
    // the pipeline sees the canonical aggregate-catalog form.
    let direct = node
        .children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .find(|t| {
            matches!(
                t.kind(),
                SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT | SyntaxKind::COUNT_KW
            )
        })
        .map(|t| {
            if t.kind() == SyntaxKind::COUNT_KW {
                t.text().to_ascii_lowercase()
            } else {
                t.text().to_string()
            }
        });
    if direct.is_some() {
        return direct;
    }
    // If wrapped in a NAME node, descend one level.
    node.children()
        .find(|n| n.kind() == SyntaxKind::NAME)
        .and_then(|name_node| {
            name_node
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
                .map(|t| t.text().to_string())
        })
}

/// Lower a `REL_LENGTH` node into [`RelLength::Variable`]. The node
/// contains a `STAR` token followed by up to two `INT_LITERAL` tokens
/// separated by a `DOT_DOT` token. `*` alone maps to `Variable { min:
/// None, max: None }` — the "any length ≥ 1" case.
fn lower_rel_length(node: &SyntaxNode) -> RelLength {
    let has_dot_dot = has_token(node, SyntaxKind::DOT_DOT);
    let mut ints = node
        .children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|t| t.kind() == SyntaxKind::INT_LITERAL)
        .filter_map(|t| t.text().parse::<u64>().ok());
    let first = ints.next();
    let second = ints.next();
    let (min, max) = if has_dot_dot {
        // `*m..n`, `*m..`, `*..n`.
        (first, second)
    } else {
        // `*m` (fixed length m) or `*` (no bounds).
        (first, first)
    };
    // Special-case `*..n`: the `INT_LITERAL` after DOT_DOT is `first`
    // when there was no leading number. Detect by checking token order:
    // if DOT_DOT precedes the first INT_LITERAL in text order, the single
    // literal is the upper bound.
    if has_dot_dot && second.is_none() && first.is_some() {
        // Decide by first token kind after STAR.
        let mut toks = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .skip_while(|t| t.kind() != SyntaxKind::STAR);
        toks.next(); // STAR
        // Skip trivia
        let next_sig = toks.find(|t| !t.kind().is_trivia());
        if next_sig.map(|t| t.kind()) == Some(SyntaxKind::DOT_DOT) {
            return RelLength::Variable {
                min: None,
                max: first,
            };
        }
    }
    RelLength::Variable { min, max }
}

/// Collect all label / rel-type IDENT tokens from a `LABEL_EXPR` or
/// `REL_TYPE_EXPR` node.
fn collect_label_idents(node: &SyntaxNode) -> Vec<SmolStr> {
    node.children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind() == SyntaxKind::QUOTED_IDENT)
        .map(|t| SmolStr::new(t.text()))
        .collect()
}

/// Map a token kind to the corresponding [`BinOp`]. Returns `None` for
/// non-operator tokens.
fn bin_op_from_kind(kind: SyntaxKind) -> Option<BinOp> {
    Some(match kind {
        SyntaxKind::PLUS => BinOp::Add,
        SyntaxKind::MINUS => BinOp::Sub,
        SyntaxKind::STAR => BinOp::Mul,
        SyntaxKind::SLASH => BinOp::Div,
        SyntaxKind::PERCENT => BinOp::Mod,
        SyntaxKind::CARET => BinOp::Pow,
        SyntaxKind::EQ => BinOp::Eq,
        SyntaxKind::NEQ | SyntaxKind::BANG_EQ => BinOp::Neq,
        SyntaxKind::LT => BinOp::Lt,
        SyntaxKind::LE => BinOp::Le,
        SyntaxKind::GT => BinOp::Gt,
        SyntaxKind::GE => BinOp::Ge,
        SyntaxKind::AND_KW => BinOp::And,
        SyntaxKind::OR_KW => BinOp::Or,
        SyntaxKind::XOR_KW => BinOp::Xor,
        _ => return None,
    })
}

/// Lower a `LITERAL_EXPR` node (free function — does not need the context).
fn lower_literal(node: SyntaxNode) -> Expr {
    let tok = node
        .children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .find(|t| !t.kind().is_trivia());
    match tok {
        Some(t) => match t.kind() {
            SyntaxKind::INT_LITERAL => {
                let text = t.text().trim();
                text.parse::<i64>()
                    .map_or_else(|_| Expr::Unresolved(SmolStr::new(text)), Expr::Int)
            }
            SyntaxKind::FLOAT_LITERAL => {
                let text = t.text().trim();
                text.parse::<f64>()
                    .map_or_else(|_| Expr::Unresolved(SmolStr::new(text)), Expr::Float)
            }
            SyntaxKind::STRING_LITERAL => {
                // Strip surrounding quotes.
                let raw = t.text();
                let inner = strip_string_delimiters(raw);
                Expr::String(SmolStr::new(inner))
            }
            SyntaxKind::BOOL_LITERAL | SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => {
                Expr::Bool(t.text().trim().eq_ignore_ascii_case("true"))
            }
            SyntaxKind::NULL_LITERAL | SyntaxKind::NULL_KW => Expr::Null,
            _ => Expr::Unresolved(SmolStr::new(t.text().trim())),
        },
        None => Expr::Null,
    }
}

/// Lower a `PARAM_EXPR` node (free function — does not need the context).
fn lower_param(node: SyntaxNode) -> Expr {
    let tok = node
        .children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .find(|t| t.kind() == SyntaxKind::PARAM);
    tok.map_or(Expr::Null, |t| {
        // PARAM token text is `$name` or `$0` — strip leading `$`.
        let raw = t.text();
        let name = raw.trim_start_matches('$');
        Expr::Param(SmolStr::new(name))
    })
}

/// Strip the leading and trailing quote characters from a string literal
/// token. Handles `'…'` and `"…"`.
fn strip_string_delimiters(raw: &str) -> &str {
    let trimmed = raw.trim();
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: produce a stable, readable snapshot string from a Statement.
    fn render(stmt: &Statement) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        writeln!(out, "clauses: {}", stmt.clauses.len()).unwrap();
        writeln!(out, "bindings: {}", stmt.bindings.len()).unwrap();
        writeln!(out, "nodes_in_map: {}", stmt.node_count()).unwrap();
        for (i, clause) in stmt.clauses.iter().enumerate() {
            writeln!(out, "clause[{i}]: {}", clause_tag(clause)).unwrap();
        }
        for (_, b) in &stmt.bindings {
            writeln!(out, "binding: {} ({:?})", b.name, b.kind).unwrap();
        }
        out
    }

    fn clause_tag(c: &Clause) -> String {
        match c {
            Clause::Match { optional, .. } => {
                if *optional {
                    "OptionalMatch".into()
                } else {
                    "Match".into()
                }
            }
            Clause::Where { .. } => "Where".into(),
            Clause::Return {
                distinct,
                projections,
                ..
            } => format!("Return(distinct={distinct}, items={})", projections.len()),
            Clause::Unwind { .. } => "Unwind".into(),
            Clause::Create { .. } => "Create".into(),
            Clause::Merge { .. } => "Merge".into(),
            Clause::Set { .. } => "Set".into(),
            Clause::Remove { .. } => "Remove".into(),
            Clause::Delete { .. } => "Delete".into(),
            Clause::Call { .. } => "Call".into(),
            Clause::With { .. } => "With".into(),
        }
    }

    // --- MATCH / RETURN snapshots ---

    #[test]
    fn snap_match_return_basic() {
        let stmt = lower_statement("MATCH (n) RETURN n");
        insta::assert_snapshot!("match_return_basic", render(&stmt));
    }

    #[test]
    fn snap_match_labeled_node() {
        let stmt = lower_statement("MATCH (p:Person) RETURN p");
        insta::assert_snapshot!("match_labeled_node", render(&stmt));
    }

    #[test]
    fn snap_match_multi_label() {
        let stmt = lower_statement("MATCH (n:Person:Employee) RETURN n");
        insta::assert_snapshot!("match_multi_label", render(&stmt));
    }

    #[test]
    fn snap_match_where() {
        let stmt = lower_statement("MATCH (n) WHERE n.age > 18 RETURN n");
        insta::assert_snapshot!("match_where", render(&stmt));
    }

    #[test]
    fn snap_optional_match() {
        let stmt = lower_statement("OPTIONAL MATCH (a:Person) RETURN a");
        insta::assert_snapshot!("optional_match", render(&stmt));
    }

    #[test]
    fn snap_return_distinct() {
        let stmt = lower_statement("MATCH (n) RETURN DISTINCT n.name");
        insta::assert_snapshot!("return_distinct", render(&stmt));
    }

    #[test]
    fn snap_return_alias() {
        let stmt = lower_statement("MATCH (n) RETURN n.name AS name");
        insta::assert_snapshot!("return_alias", render(&stmt));
    }

    #[test]
    fn snap_return_star() {
        let stmt = lower_statement("MATCH (n) RETURN *");
        insta::assert_snapshot!("return_star", render(&stmt));
    }

    // --- Pattern snapshots ---

    #[test]
    fn snap_pattern_anonymous_node() {
        let stmt = lower_statement("MATCH () RETURN 1");
        insta::assert_snapshot!("pattern_anonymous_node", render(&stmt));
    }

    #[test]
    fn snap_pattern_node_props() {
        let stmt = lower_statement("MATCH (n:Person {name: 'Alice', age: 30}) RETURN n");
        insta::assert_snapshot!("pattern_node_props", render(&stmt));
    }

    #[test]
    fn snap_pattern_rel_directed_right() {
        let stmt = lower_statement("MATCH (a)-[:KNOWS]->(b) RETURN a, b");
        insta::assert_snapshot!("pattern_rel_directed_right", render(&stmt));
    }

    #[test]
    fn snap_pattern_rel_directed_left() {
        let stmt = lower_statement("MATCH (a)<-[:KNOWS]-(b) RETURN a, b");
        insta::assert_snapshot!("pattern_rel_directed_left", render(&stmt));
    }

    #[test]
    fn snap_pattern_rel_undirected() {
        let stmt = lower_statement("MATCH (a)-[r]-(b) RETURN a, b");
        insta::assert_snapshot!("pattern_rel_undirected", render(&stmt));
    }

    #[test]
    fn snap_pattern_chain_three_nodes() {
        let stmt = lower_statement("MATCH (a)-[:R1]->(b)-[:R2]->(c) RETURN a, b, c");
        insta::assert_snapshot!("pattern_chain_three_nodes", render(&stmt));
    }

    #[test]
    fn snap_pattern_rel_with_props() {
        let stmt = lower_statement("MATCH (a)-[r:KNOWS {since: 2020}]->(b) RETURN r");
        insta::assert_snapshot!("pattern_rel_with_props", render(&stmt));
    }

    // --- Expression snapshots ---

    #[test]
    fn snap_expr_literal_int() {
        let stmt = lower_statement("RETURN 42");
        insta::assert_snapshot!("expr_literal_int", render(&stmt));
    }

    #[test]
    fn snap_expr_literal_float() {
        let stmt = lower_statement("RETURN 3.14");
        insta::assert_snapshot!("expr_literal_float", render(&stmt));
    }

    #[test]
    fn snap_expr_literal_string() {
        let stmt = lower_statement("RETURN 'hello'");
        insta::assert_snapshot!("expr_literal_string", render(&stmt));
    }

    #[test]
    fn snap_expr_literal_bool() {
        let stmt = lower_statement("RETURN TRUE");
        insta::assert_snapshot!("expr_literal_bool", render(&stmt));
    }

    #[test]
    fn snap_expr_literal_null() {
        let stmt = lower_statement("RETURN NULL");
        insta::assert_snapshot!("expr_literal_null", render(&stmt));
    }

    #[test]
    fn snap_expr_parameter() {
        let stmt = lower_statement("RETURN $name");
        insta::assert_snapshot!("expr_parameter", render(&stmt));
    }

    #[test]
    fn snap_expr_prop_access() {
        let stmt = lower_statement("MATCH (n) RETURN n.name");
        insta::assert_snapshot!("expr_prop_access", render(&stmt));
    }

    #[test]
    fn snap_expr_binary_arithmetic() {
        let stmt = lower_statement("RETURN 1 + 2 * 3");
        insta::assert_snapshot!("expr_binary_arithmetic", render(&stmt));
    }

    #[test]
    fn snap_expr_binary_comparison() {
        let stmt = lower_statement("MATCH (n) WHERE n.age >= 18 RETURN n");
        insta::assert_snapshot!("expr_binary_comparison", render(&stmt));
    }

    #[test]
    fn snap_expr_unary_not() {
        let stmt = lower_statement("RETURN NOT TRUE");
        insta::assert_snapshot!("expr_unary_not", render(&stmt));
    }

    #[test]
    fn snap_expr_unary_minus() {
        let stmt = lower_statement("RETURN -1");
        insta::assert_snapshot!("expr_unary_minus", render(&stmt));
    }

    #[test]
    fn snap_expr_is_null() {
        let stmt = lower_statement("RETURN NULL IS NULL");
        insta::assert_snapshot!("expr_is_null", render(&stmt));
    }

    #[test]
    fn snap_expr_is_not_null() {
        let stmt = lower_statement("RETURN NULL IS NOT NULL");
        insta::assert_snapshot!("expr_is_not_null", render(&stmt));
    }

    #[test]
    fn snap_expr_in_list() {
        let stmt = lower_statement("MATCH (n) WHERE n.age IN n.ages RETURN n");
        insta::assert_snapshot!("expr_in_list", render(&stmt));
    }

    #[test]
    fn snap_expr_function_call() {
        let stmt = lower_statement("RETURN count(n)");
        insta::assert_snapshot!("expr_function_call", render(&stmt));
    }

    #[test]
    fn snap_expr_function_call_distinct() {
        let stmt = lower_statement("MATCH (n) RETURN count(DISTINCT n)");
        insta::assert_snapshot!("expr_function_call_distinct", render(&stmt));
    }

    /// `count(*)` lowers to `Expr::Call { name: "count", args: [], .. }`.
    /// `lg-query-cyrs`'s `AggAccumulator::new` detects `count_star` from
    /// `func == "count" && args.is_empty()`, so the empty-args lowering is
    /// the executor-side contract.
    #[test]
    fn lower_count_star_has_empty_args() {
        let stmt = lower_statement("RETURN count(*)");
        let Clause::Return { projections, .. } = &stmt.clauses[0] else {
            panic!("expected Return clause, got {:?}", stmt.clauses[0]);
        };
        assert_eq!(projections.len(), 1);
        match &projections[0].expr {
            Expr::Call {
                name,
                args,
                distinct,
            } => {
                assert_eq!(name.as_str(), "count");
                assert!(args.is_empty(), "count(*) should lower to empty args");
                assert!(!*distinct);
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }
    }

    #[test]
    fn snap_expr_starts_with() {
        let stmt = lower_statement("RETURN a STARTS WITH 'foo'");
        insta::assert_snapshot!("expr_starts_with", render(&stmt));
    }

    #[test]
    fn snap_expr_regex_match() {
        let stmt = lower_statement("RETURN a =~ 'r.*'");
        insta::assert_snapshot!("expr_regex_match", render(&stmt));
    }

    // --- cy-ypm: clause ordering regression tests (spec §6.1) ---
    //
    // MATCH (a) WHERE ... RETURN a must lower to clause order
    // [Match, Where, Return] — i.e. source order.  A prior bug
    // (cy-lve diagnosis) pushed an inline WHERE before its owning
    // MATCH, which orphaned Filter on EMPTY_SOURCE in plan lowering.

    fn clause_kinds(stmt: &Statement) -> Vec<&'static str> {
        stmt.clauses
            .iter()
            .map(|c| match c {
                Clause::Match {
                    optional: false, ..
                } => "Match",
                Clause::Match { optional: true, .. } => "OptionalMatch",
                Clause::Where { .. } => "Where",
                Clause::Return { .. } => "Return",
                Clause::With { .. } => "With",
                Clause::Unwind { .. } => "Unwind",
                Clause::Create { .. } => "Create",
                Clause::Merge { .. } => "Merge",
                Clause::Set { .. } => "Set",
                Clause::Remove { .. } => "Remove",
                Clause::Delete { .. } => "Delete",
                Clause::Call { .. } => "Call",
            })
            .collect()
    }

    #[test]
    fn match_where_return_preserves_source_order() {
        let stmt = lower_statement("MATCH (a) WHERE n.x = 1 RETURN a");
        assert_eq!(
            clause_kinds(&stmt),
            vec!["Match", "Where", "Return"],
            "inline WHERE must follow its owning MATCH (cy-ypm)",
        );
    }

    #[test]
    fn optional_match_where_return_preserves_source_order() {
        let stmt = lower_statement("OPTIONAL MATCH (a) WHERE a.x = 1 RETURN a");
        assert_eq!(
            clause_kinds(&stmt),
            vec!["OptionalMatch", "Where", "Return"],
            "inline WHERE must follow its owning OPTIONAL MATCH (cy-ypm)",
        );
    }

    #[test]
    fn two_matches_each_with_inline_where() {
        let stmt = lower_statement("MATCH (a) WHERE a.x = 1 MATCH (b) WHERE b.y = 2 RETURN a, b");
        assert_eq!(
            clause_kinds(&stmt),
            vec!["Match", "Where", "Match", "Where", "Return"],
            "each inline WHERE must follow its own MATCH (cy-ypm)",
        );
    }

    // --- node_map / binding correctness tests ---

    #[test]
    fn node_map_populated_for_match_return() {
        let stmt = lower_statement("MATCH (n) RETURN n");
        assert!(stmt.node_count() >= 2, "at least MATCH + RETURN HirIds");
    }

    #[test]
    fn var_id_assigned_for_bound_variable() {
        let stmt = lower_statement("MATCH (person) RETURN person");
        assert_eq!(stmt.bindings.len(), 1);
        let b = stmt.bindings.values().next().unwrap();
        assert_eq!(b.name.as_str(), "person");
        assert_eq!(b.kind, VarKind::Node);
    }

    #[test]
    fn rel_var_id_assigned() {
        let stmt = lower_statement("MATCH (a)-[r:KNOWS]->(b) RETURN r");
        let kinds: Vec<VarKind> = stmt.bindings.values().map(|b| b.kind).collect();
        assert!(kinds.contains(&VarKind::Relationship));
        assert!(kinds.contains(&VarKind::Node));
    }

    // --- ORDER BY / SKIP / LIMIT trailer tests (lg-q9m) ---

    fn last_return_trailers(stmt: &Statement) -> (&[OrderItem], Option<&Expr>, Option<&Expr>) {
        let ret = stmt
            .clauses
            .iter()
            .rev()
            .find(|c| matches!(c, Clause::Return { .. }))
            .expect("expected at least one RETURN clause");
        match ret {
            Clause::Return {
                order_by,
                skip,
                limit,
                ..
            } => (order_by.as_slice(), skip.as_ref(), limit.as_ref()),
            _ => unreachable!(),
        }
    }

    #[test]
    fn return_with_limit_lowers_limit_field() {
        let stmt = lower_statement("MATCH (n) RETURN n LIMIT 1");
        let (order_by, skip, limit) = last_return_trailers(&stmt);
        assert!(order_by.is_empty(), "no ORDER BY expected");
        assert!(skip.is_none(), "no SKIP expected");
        assert!(
            matches!(limit, Some(Expr::Int(1))),
            "LIMIT 1 should lower to Expr::Int(1), got {limit:?}",
        );
    }

    #[test]
    fn return_with_order_skip_limit_lowers_all_three() {
        let stmt = lower_statement("MATCH (n) RETURN n ORDER BY n.age DESC SKIP 2 LIMIT 3");
        let (order_by, skip, limit) = last_return_trailers(&stmt);

        assert_eq!(order_by.len(), 1, "one ORDER BY item expected");
        assert!(
            order_by[0].descending,
            "DESC keyword should set descending=true",
        );
        match &order_by[0].expr {
            Expr::Prop { prop, .. } => assert_eq!(prop.as_str(), "age"),
            other => panic!("expected Expr::Prop for n.age, got {other:?}"),
        }

        assert!(
            matches!(skip, Some(Expr::Int(2))),
            "SKIP 2 should lower to Expr::Int(2), got {skip:?}",
        );
        assert!(
            matches!(limit, Some(Expr::Int(3))),
            "LIMIT 3 should lower to Expr::Int(3), got {limit:?}",
        );
    }

    #[test]
    fn return_without_trailers_has_empty_modifiers() {
        let stmt = lower_statement("MATCH (n) RETURN n");
        let (order_by, skip, limit) = last_return_trailers(&stmt);
        assert!(order_by.is_empty());
        assert!(skip.is_none());
        assert!(limit.is_none());
    }

    #[test]
    fn return_order_by_default_direction_is_ascending() {
        let stmt = lower_statement("MATCH (n) RETURN n ORDER BY n.age");
        let (order_by, _, _) = last_return_trailers(&stmt);
        assert_eq!(order_by.len(), 1);
        assert!(
            !order_by[0].descending,
            "absent direction keyword should default to ascending",
        );
    }

    #[test]
    fn with_clause_carries_trailers() {
        // WITH supports the same ORDER BY / SKIP / LIMIT trailer.
        let stmt = lower_statement("MATCH (n) WITH n ORDER BY n.name SKIP 5 LIMIT 10 RETURN n");
        let with = stmt
            .clauses
            .iter()
            .find(|c| matches!(c, Clause::With { .. }))
            .expect("expected a WITH clause");
        let Clause::With {
            order_by,
            skip,
            limit,
            ..
        } = with
        else {
            unreachable!();
        };
        assert_eq!(order_by.len(), 1);
        assert!(!order_by[0].descending);
        assert!(matches!(skip, Some(Expr::Int(5))));
        assert!(matches!(limit, Some(Expr::Int(10))));
    }
}
