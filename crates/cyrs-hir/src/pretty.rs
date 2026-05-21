//! HIR pretty-printer with resolved-name overlay (spec 0001 §17.2 / §6.2).
//!
//! [`print_overlay`] renders a [`Statement`] as a human-readable tree where
//! every variable reference is annotated with its resolution:
//!
//! ```text
//! Var(0)[n←Node@0..9]
//! ```
//!
//! Format key:
//! - `Var(N)` — the raw [`VarId`] as lowered.
//! - `[name←Kind@offset..end]` — resolved: binding name, [`VarKind`], and the
//!   `defined_at` byte range from the [`Binding`].
//! - `[name?Unresolved]` — the reference could not be resolved (E1001 site).
//! - `Unresolved("x")` — an [`Expr::Unresolved`] that survived lowering and
//!   still has no `VarId`; the raw name is shown.
//!
//! Determinism: the renderer walks the HIR in tree order (no `HashMap`
//! iteration) and uses `IndexMap` from the [`Statement`] and
//! [`ResolvedNames`], both of which preserve insertion order (AGENTS.md §8).

use std::fmt::Write as _;

use crate::{
    BinOp, Binding, Clause, Direction, Expr, MapProjectionItem, Pattern, PatternElement,
    PatternPart, Projection, RelLength, RemoveItem, SetItem, Statement, UnaryOp, VarId, VarKind,
    YieldItem,
    scope::{BindingKind, Resolution, ResolvedNames},
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Pretty-print `stmt` with a resolved-name overlay derived from
/// `resolved`.  Returns a multi-line `String`.
///
/// Every `Expr::Var(id)` is rendered as `Var(N)[name←Kind@from..to]` when
/// `resolved` contains a successful resolution for `id`, or as
/// `Var(N)[name?Unresolved]` when unresolved. Variables that appear only at
/// binding sites (pattern elements) are shown with their kind annotation
/// inline.
pub fn print_overlay(stmt: &Statement, resolved: &ResolvedNames) -> String {
    let ctx = OverlayCtx {
        bindings: &stmt.bindings,
        resolved,
    };
    let mut out = String::new();
    ctx.print_statement(stmt, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Internal context
// ---------------------------------------------------------------------------

struct OverlayCtx<'a> {
    bindings: &'a indexmap::IndexMap<VarId, Binding>,
    resolved: &'a ResolvedNames,
}

impl OverlayCtx<'_> {
    // --- lookup helpers -----------------------------------------------------

    fn binding_of(&self, var: VarId) -> Option<&Binding> {
        self.bindings.get(&var)
    }

    /// Format a variable that appears at a use site (inside an expression).
    ///
    /// Output examples:
    /// - `Var(0)[n←Node@0..9]`     — resolved
    /// - `Var(0)[n?Unresolved]`    — binding exists in stmt but not resolved
    fn fmt_var_use(&self, var: VarId) -> String {
        let id_str = format!("Var({})", var.0);
        let annotation = match self.resolved.get(var) {
            Some(Resolution::Resolved(rb)) => {
                let def = self.binding_of(rb.defining_var);
                let name = def.map_or_else(
                    || format!("__var{}", rb.defining_var.0),
                    |b| b.name.to_string(),
                );
                let kind = kind_str(&rb.kind);
                let span = def.map_or_else(
                    || "?..?".to_string(),
                    |b| {
                        format!(
                            "{}..{}",
                            u32::from(b.defined_at.start()),
                            u32::from(b.defined_at.end())
                        )
                    },
                );
                format!("[{name}←{kind}@{span}]")
            }
            Some(Resolution::Unresolved) => {
                let name = self
                    .binding_of(var)
                    .map_or_else(|| format!("__var{}", var.0), |b| b.name.to_string());
                format!("[{name}?Unresolved]")
            }
            None => {
                // Variable exists in bindings but resolve() was not called on it;
                // annotate from the bindings map directly.
                match self.binding_of(var) {
                    Some(b) => {
                        let kind = varkind_str(b.kind);
                        let span = format!(
                            "{}..{}",
                            u32::from(b.defined_at.start()),
                            u32::from(b.defined_at.end())
                        );
                        format!("[{}←{kind}@{span}]", b.name)
                    }
                    None => "[?]".to_string(),
                }
            }
        };
        format!("{id_str}{annotation}")
    }

    /// Format a binding site variable (pattern binder).
    /// Always shows the name and kind from `Statement::bindings`.
    fn fmt_var_bind(&self, var: VarId) -> String {
        match self.binding_of(var) {
            Some(b) => {
                let kind = varkind_str(b.kind);
                let span = format!(
                    "{}..{}",
                    u32::from(b.defined_at.start()),
                    u32::from(b.defined_at.end())
                );
                format!("{}[bind:Var({})←{kind}@{span}]", b.name, var.0)
            }
            None => format!("__var{}[bind:?]", var.0),
        }
    }

    // --- statement ----------------------------------------------------------

    fn print_statement(&self, stmt: &Statement, out: &mut String) {
        writeln!(out, "Statement {{").unwrap();
        writeln!(
            out,
            "  span: {}..{}",
            u32::from(stmt.span.start()),
            u32::from(stmt.span.end())
        )
        .unwrap();
        writeln!(out, "  bindings: [").unwrap();
        for (_, b) in &stmt.bindings {
            let kind = varkind_str(b.kind);
            writeln!(
                out,
                "    Var({}) {}: {} ({kind}) @{}..{}",
                b.id.0,
                b.id.0,
                b.name,
                u32::from(b.defined_at.start()),
                u32::from(b.defined_at.end())
            )
            .unwrap();
        }
        writeln!(out, "  ]").unwrap();
        writeln!(out, "  clauses: [").unwrap();
        for clause in &stmt.clauses {
            self.print_clause(clause, out, 2);
        }
        writeln!(out, "  ]").unwrap();
        writeln!(out, "}}").unwrap();
    }

    // --- clauses ------------------------------------------------------------

    fn print_clause(&self, clause: &Clause, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        match clause {
            Clause::Match {
                optional,
                pattern,
                span,
                ..
            } => {
                let kw = if *optional { "OptionalMatch" } else { "Match" };
                writeln!(
                    out,
                    "{pad}{kw} @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                self.print_pattern(pattern, out, indent + 1);
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Where {
                predicate, span, ..
            } => {
                writeln!(
                    out,
                    "{pad}Where @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                let expr_str = self.fmt_expr(predicate, indent + 1);
                writeln!(out, "{pad}  predicate: {expr_str}").unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::With {
                projections,
                filter,
                span,
                ..
            } => {
                writeln!(
                    out,
                    "{pad}With @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                self.print_projections(projections, out, indent + 1);
                if let Some(f) = filter {
                    let filter_str = self.fmt_expr(f, indent + 1);
                    writeln!(out, "{pad}  filter: {filter_str}").unwrap();
                }
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Return {
                projections,
                distinct,
                span,
                ..
            } => {
                writeln!(
                    out,
                    "{pad}Return(distinct={distinct}) @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                self.print_projections(projections, out, indent + 1);
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Unwind {
                list, bind, span, ..
            } => {
                writeln!(
                    out,
                    "{pad}Unwind @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                let list_str = self.fmt_expr(list, indent + 1);
                writeln!(out, "{pad}  list: {list_str}").unwrap();
                let bind_str = self.fmt_var_bind(*bind);
                writeln!(out, "{pad}  bind: {bind_str}").unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Create { pattern, span, .. } => {
                writeln!(
                    out,
                    "{pad}Create @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                self.print_pattern(pattern, out, indent + 1);
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Merge {
                pattern,
                on_create,
                on_match,
                span,
                ..
            } => {
                writeln!(
                    out,
                    "{pad}Merge @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                self.print_pattern(pattern, out, indent + 1);
                if !on_create.is_empty() {
                    let n = on_create.len();
                    writeln!(out, "{pad}  on_create: {n} items").unwrap();
                }
                if !on_match.is_empty() {
                    let n = on_match.len();
                    writeln!(out, "{pad}  on_match: {n} items").unwrap();
                }
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Set { items, span, .. } => {
                writeln!(
                    out,
                    "{pad}Set @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                for item in items {
                    self.print_set_item(item, out, indent + 1);
                }
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Remove { items, span, .. } => {
                writeln!(
                    out,
                    "{pad}Remove @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                for item in items {
                    self.print_remove_item(item, out, indent + 1);
                }
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Delete {
                targets,
                detach,
                span,
                ..
            } => {
                writeln!(
                    out,
                    "{pad}Delete(detach={detach}) @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                for t in targets {
                    let ts = self.fmt_expr(t, indent + 1);
                    writeln!(out, "{pad}  {ts}").unwrap();
                }
                writeln!(out, "{pad}}}").unwrap();
            }
            Clause::Call {
                procedure,
                args,
                yields,
                span,
                ..
            } => {
                writeln!(
                    out,
                    "{pad}Call({procedure}) @{}..{} {{",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
                for a in args {
                    let as_ = self.fmt_expr(a, indent + 1);
                    writeln!(out, "{pad}  arg: {as_}").unwrap();
                }
                for y in yields {
                    Self::print_yield(y, out, indent + 1);
                }
                writeln!(out, "{pad}}}").unwrap();
            }
        }
    }

    // --- pattern helpers ----------------------------------------------------

    fn print_pattern(&self, pattern: &Pattern, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        writeln!(out, "{pad}Pattern {{").unwrap();
        for (i, part) in pattern.parts.iter().enumerate() {
            self.print_pattern_part(part, out, indent + 1, i);
        }
        writeln!(out, "{pad}}}").unwrap();
    }

    fn print_pattern_part(&self, part: &PatternPart, out: &mut String, indent: usize, idx: usize) {
        let pad = "  ".repeat(indent);
        let named = part
            .named_as
            .map_or_else(String::new, |v| format!(" as={}", self.fmt_var_bind(v)));
        writeln!(out, "{pad}part[{idx}]{named} [").unwrap();
        for elem in &part.elements {
            self.print_pattern_elem(elem, out, indent + 1);
        }
        writeln!(out, "{pad}]").unwrap();
    }

    fn print_pattern_elem(&self, elem: &PatternElement, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        match elem {
            PatternElement::Node {
                bind,
                labels,
                props,
                span,
                ..
            } => {
                let bind_str = bind.map_or_else(|| "_anon".to_string(), |v| self.fmt_var_bind(v));
                let labels_str = if labels.is_empty() {
                    String::new()
                } else {
                    format!(":{}", labels.join(":"))
                };
                write!(out, "{pad}Node({bind_str}{labels_str})").unwrap();
                if let Some(p) = props {
                    let ps = self.fmt_expr(p, indent);
                    write!(out, " props={ps}").unwrap();
                }
                writeln!(
                    out,
                    " @{}..{}",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
            }
            PatternElement::Rel {
                bind,
                types,
                direction,
                length,
                props,
                span,
                ..
            } => {
                let bind_str = bind.map_or_else(|| "_anon".to_string(), |v| self.fmt_var_bind(v));
                let dir = match direction {
                    Direction::Outgoing => "->",
                    Direction::Incoming => "<-",
                    Direction::Undirected => "--",
                };
                let type_str = if types.is_empty() {
                    String::new()
                } else {
                    format!(":{}", types.join("|"))
                };
                let len_str = match length {
                    RelLength::Single => String::new(),
                    RelLength::Variable { min, max } => {
                        let lo = min.map_or_else(String::new, |n| n.to_string());
                        let hi = max.map_or_else(String::new, |n| n.to_string());
                        format!("*{lo}..{hi}")
                    }
                };
                write!(out, "{pad}Rel({dir} {bind_str}{type_str}{len_str})").unwrap();
                if let Some(p) = props {
                    let ps = self.fmt_expr(p, indent);
                    write!(out, " props={ps}").unwrap();
                }
                writeln!(
                    out,
                    " @{}..{}",
                    u32::from(span.start()),
                    u32::from(span.end())
                )
                .unwrap();
            }
        }
    }

    // --- projection helpers -------------------------------------------------

    fn print_projections(&self, projs: &[Projection], out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        for (i, p) in projs.iter().enumerate() {
            let expr_str = self.fmt_expr(&p.expr, indent);
            let alias = p
                .alias
                .as_ref()
                .map_or_else(String::new, |a| format!(" AS {a}"));
            writeln!(out, "{pad}proj[{i}]: {expr_str}{alias}").unwrap();
        }
    }

    // --- set/remove helpers -------------------------------------------------

    fn print_set_item(&self, item: &SetItem, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        match item {
            SetItem::Property {
                target,
                prop,
                value,
            } => {
                let ts = self.fmt_expr(target, indent);
                let vs = self.fmt_expr(value, indent);
                writeln!(out, "{pad}Set.Prop: {ts}.{prop} = {vs}").unwrap();
            }
            SetItem::Labels { target, labels } => {
                let ts = self.fmt_var_use(*target);
                writeln!(out, "{pad}Set.Labels: {ts} :{}", labels.join(":")).unwrap();
            }
            SetItem::AssignMap {
                target,
                map,
                replace,
            } => {
                let ts = self.fmt_var_use(*target);
                let ms = self.fmt_expr(map, indent);
                let op = if *replace { "=" } else { "+=" };
                writeln!(out, "{pad}Set.Map: {ts} {op} {ms}").unwrap();
            }
        }
    }

    fn print_remove_item(&self, item: &RemoveItem, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        match item {
            RemoveItem::Property { target, prop } => {
                let ts = self.fmt_expr(target, indent);
                writeln!(out, "{pad}Remove.Prop: {ts}.{prop}").unwrap();
            }
            RemoveItem::Labels { target, labels } => {
                let ts = self.fmt_var_use(*target);
                writeln!(out, "{pad}Remove.Labels: {ts} :{}", labels.join(":")).unwrap();
            }
        }
    }

    fn print_yield(yi: &YieldItem, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        let alias = yi
            .alias
            .as_ref()
            .map_or_else(String::new, |a| format!(" AS {a}"));
        let name = &yi.name;
        writeln!(out, "{pad}yield: {name}{alias}").unwrap();
    }

    // --- expression ---------------------------------------------------------

    /// Render an expression as a compact single-line string.
    /// Variable uses are annotated with the overlay.
    fn fmt_expr(&self, expr: &Expr, _indent: usize) -> String {
        match expr {
            Expr::Null => "null".to_string(),
            Expr::Bool(b) => b.to_string(),
            Expr::Int(i) => i.to_string(),
            Expr::Float(f) => format!("{f:?}"),
            Expr::String(s) => format!("{s:?}"),
            Expr::Var(v) => self.fmt_var_use(*v),
            Expr::Param(p) => format!("${p}"),
            Expr::Prop { target, prop } => {
                format!("{}.{prop}", self.fmt_expr(target, 0))
            }
            Expr::Index { target, index } => {
                format!("{}[{}]", self.fmt_expr(target, 0), self.fmt_expr(index, 0))
            }
            Expr::Slice { target, start, end } => {
                let s = start
                    .as_ref()
                    .map_or_else(String::new, |e| self.fmt_expr(e, 0));
                let e = end
                    .as_ref()
                    .map_or_else(String::new, |e| self.fmt_expr(e, 0));
                format!("{}[{s}..{e}]", self.fmt_expr(target, 0))
            }
            Expr::List(items) => {
                let inner: Vec<_> = items.iter().map(|e| self.fmt_expr(e, 0)).collect();
                format!("[{}]", inner.join(", "))
            }
            Expr::Map(pairs) => {
                let inner: Vec<_> = pairs
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", self.fmt_expr(v, 0)))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Expr::Call {
                name,
                args,
                distinct,
            } => {
                let inner: Vec<_> = args.iter().map(|e| self.fmt_expr(e, 0)).collect();
                let d = if *distinct { "DISTINCT " } else { "" };
                format!("{name}({d}{})", inner.join(", "))
            }
            Expr::BinOp { op, lhs, rhs } => {
                format!(
                    "({} {} {})",
                    self.fmt_expr(lhs, 0),
                    fmt_binop(*op),
                    self.fmt_expr(rhs, 0)
                )
            }
            Expr::UnaryOp { op, operand } => {
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "NOT ",
                };
                format!("{op_str}{}", self.fmt_expr(operand, 0))
            }
            Expr::IsNull { operand, negated } => {
                let neg = if *negated { " NOT" } else { "" };
                format!("{} IS{neg} NULL", self.fmt_expr(operand, 0))
            }
            Expr::InList { operand, list } => {
                format!(
                    "{} IN {}",
                    self.fmt_expr(operand, 0),
                    self.fmt_expr(list, 0)
                )
            }
            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => {
                let s = scrutinee
                    .as_ref()
                    .map_or_else(String::new, |e| format!(" {}", self.fmt_expr(e, 0)));
                let arm_strs: Vec<_> = arms
                    .iter()
                    .map(|(c, t)| {
                        format!("WHEN {} THEN {}", self.fmt_expr(c, 0), self.fmt_expr(t, 0))
                    })
                    .collect();
                let else_str = otherwise
                    .as_ref()
                    .map_or_else(String::new, |e| format!(" ELSE {}", self.fmt_expr(e, 0)));
                format!("CASE{s} {}{else_str} END", arm_strs.join(" "))
            }
            Expr::PatternPredicate(pat) => {
                let parts: Vec<_> = pat
                    .parts
                    .iter()
                    .map(|p| {
                        let elems: Vec<_> = p
                            .elements
                            .iter()
                            .map(|e| match e {
                                PatternElement::Node { bind, labels, .. } => {
                                    let b = bind.map_or_else(
                                        || "()".to_string(),
                                        |v| format!("({})", self.fmt_var_bind(v)),
                                    );
                                    let l = if labels.is_empty() {
                                        String::new()
                                    } else {
                                        format!(":{}", labels.join(":"))
                                    };
                                    format!("{b}{l}")
                                }
                                PatternElement::Rel {
                                    bind,
                                    types,
                                    direction,
                                    ..
                                } => {
                                    let b = bind.map_or_else(
                                        || "[_]".to_string(),
                                        |v| format!("[{}]", self.fmt_var_bind(v)),
                                    );
                                    let t = if types.is_empty() {
                                        String::new()
                                    } else {
                                        format!(":{}", types.join("|"))
                                    };
                                    let d = match direction {
                                        Direction::Outgoing => "->",
                                        Direction::Incoming => "<-",
                                        Direction::Undirected => "--",
                                    };
                                    format!("{d}{b}{t}")
                                }
                            })
                            .collect();
                        elems.join("")
                    })
                    .collect();
                format!("PatternPred({})", parts.join(", "))
            }
            Expr::ListComprehension {
                filter_var,
                iterable,
                filter,
                map_expr,
            } => {
                let var_str = self.fmt_var_bind(*filter_var);
                let iter_str = self.fmt_expr(iterable, 0);
                let filter_str = filter
                    .as_ref()
                    .map_or_else(String::new, |f| format!(" WHERE {}", self.fmt_expr(f, 0)));
                let map_str = self.fmt_expr(map_expr, 0);
                format!("[{var_str} IN {iter_str}{filter_str} | {map_str}]")
            }
            Expr::ListPredicate {
                kind,
                var,
                iterable,
                predicate,
            } => {
                let kw = match kind {
                    crate::ListPredKind::Any => "ANY",
                    crate::ListPredKind::All => "ALL",
                    crate::ListPredKind::None => "NONE",
                    crate::ListPredKind::Single => "SINGLE",
                };
                let var_str = self.fmt_var_bind(*var);
                let iter_str = self.fmt_expr(iterable, 0);
                let pred_str = predicate
                    .as_ref()
                    .map_or_else(String::new, |p| format!(" WHERE {}", self.fmt_expr(p, 0)));
                format!("{kw}({var_str} IN {iter_str}{pred_str})")
            }
            Expr::MapProjection { base, items } => {
                let base_str = self.fmt_expr(base, 0);
                let item_strs: Vec<_> = items
                    .iter()
                    .map(|item| self.fmt_map_proj_item(item))
                    .collect();
                format!("{base_str} {{{}}}", item_strs.join(", "))
            }
            Expr::Unresolved(name) => format!("Unresolved({name:?})"),
            // cy-p1u5: parser-only EXISTS subquery marker. Pretty as a
            // stable string with no body — sema rejects this shape
            // before it can be evaluated.
            Expr::ExistsSubqueryDeferred { .. } => "ExistsSubqueryDeferred".to_string(),
        }
    }

    fn fmt_map_proj_item(&self, item: &MapProjectionItem) -> String {
        match item {
            MapProjectionItem::PropCopy { prop } => format!(".{prop}"),
            MapProjectionItem::Computed { key, value } => {
                format!("{key}: {}", self.fmt_expr(value, 0))
            }
            MapProjectionItem::VarShorthand { var, name } => {
                format!("{name}({})", self.fmt_var_bind(*var))
            }
            MapProjectionItem::Aliased { key, value } => {
                format!("{key} AS {}", self.fmt_expr(value, 0))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn kind_str(k: &BindingKind) -> &'static str {
    match k {
        BindingKind::Node => "Node",
        BindingKind::Relationship => "Relationship",
        BindingKind::Path => "Path",
        BindingKind::Value => "Value",
    }
}

fn varkind_str(k: VarKind) -> &'static str {
    match k {
        VarKind::Node => "Node",
        VarKind::Relationship => "Relationship",
        VarKind::Path => "Path",
        VarKind::Value => "Value",
    }
}

fn fmt_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add | BinOp::Concat => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "^",
        BinOp::Eq => "=",
        BinOp::Neq => "<>",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "AND",
        BinOp::Or => "OR",
        BinOp::Xor => "XOR",
        BinOp::StartsWith => "STARTS WITH",
        BinOp::EndsWith => "ENDS WITH",
        BinOp::Contains => "CONTAINS",
        BinOp::RegexMatch => "=~",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Binding, Clause, Pattern, PatternElement, PatternPart, Projection, Statement, VarId,
        VarKind,
        scope::{BindingKind, Resolution, ResolvedBinding, ResolvedNames},
    };
    use cyrs_syntax::{TextRange, TextSize};
    use smol_str::SmolStr;

    fn small_range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    fn dummy_syntax() -> cyrs_syntax::SyntaxNode {
        cyrs_syntax::parse("x").syntax()
    }

    fn alloc(stmt: &mut Statement) -> crate::HirId {
        stmt.alloc_id(dummy_syntax())
    }

    fn intern_var(stmt: &mut Statement, name: &str, kind: VarKind, span_end: u32) -> VarId {
        for (id, b) in &stmt.bindings {
            if b.name.as_str() == name {
                return *id;
            }
        }
        let id = VarId(u32::try_from(stmt.bindings.len()).expect("overflow"));
        stmt.bindings.insert(
            id,
            Binding {
                id,
                name: SmolStr::new(name),
                kind,
                defined_at: small_range(0, span_end),
            },
        );
        id
    }

    // Simple resolved-names helper: register a var as resolved to itself.
    fn resolve_self(resolved: &mut ResolvedNames, var: VarId, kind: BindingKind) {
        resolved.insert(
            var,
            Resolution::Resolved(ResolvedBinding {
                defining_var: var,
                kind,
                via_projection: false,
            }),
        );
    }

    #[test]
    fn snap_overlay_simple_match_return() {
        let mut stmt = Statement::new(small_range(0, 18));
        let n = intern_var(&mut stmt, "n", VarKind::Node, 9);

        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    shortest: crate::ShortestPath::No,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(n),
                        labels: vec![],
                        props: None,
                        span: small_range(6, 9),
                    }],
                }],
            },
            span: small_range(0, 9),
        });
        let rid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: rid,
            projections: vec![Projection {
                expr: Expr::Var(n),
                alias: None,
                span: small_range(17, 18),
            }],
            distinct: false,
            order_by: Vec::new(),
            skip: None,
            limit: None,
            span: small_range(10, 18),
        });

        let mut resolved = ResolvedNames::new();
        resolve_self(&mut resolved, n, BindingKind::Node);

        insta::assert_snapshot!(
            "overlay_simple_match_return",
            print_overlay(&stmt, &resolved)
        );
    }
}
