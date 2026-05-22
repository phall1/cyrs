//! Human-readable pretty-printer for the plan IR (spec 0001 §12).
//!
//! Entry point: [`pretty`].
//!
//! Output style: tree-indented with 2-space indent per level. Operator name
//! appears on the first line; key fields follow inline where concise.
//!
//! Example:
//! ```text
//! Project [n.name AS name]
//!   Filter (n.age > 18)
//!     Source (:Person AS $0)
//! ```

use std::fmt::Write as _;

use crate::lower::PlanStatement;
use crate::{
    AggExpr, BinOp, Direction, Expr, LabelSet, ListPredKind, OrderKey, Projection, ReadOp,
    RelLength, RelSpec, SortDir, UnaryOp, UnionKind, WriteOp,
};

const INDENT: &str = "  ";

/// Render a [`PlanStatement`] as a human-readable tree string.
///
/// The plan's `ops` arena is used to reconstruct the tree: the last element is
/// the root. Write ops (if any) are appended after the read tree.
///
/// Spec §12.1 — pretty-print style.
pub fn pretty(plan: &PlanStatement) -> String {
    let mut out = String::new();

    if plan.ops.is_empty() && plan.write_ops.is_empty() {
        out.push_str("EmptyPlan\n");
        return out;
    }

    // Print read ops tree starting from root (last op).
    if !plan.ops.is_empty() {
        let root_idx = plan.ops.len() - 1;
        print_op(&plan.ops, root_idx, 0, &mut out);
    }

    // Print write ops.
    for (i, wop) in plan.write_ops.iter().enumerate() {
        if i == 0 {
            writeln!(out, "WriteOps:").unwrap();
        }
        let mut wline = String::new();
        print_write_op(wop, 1, &mut wline);
        out.push_str(&wline);
    }

    out
}

fn indent(depth: usize) -> String {
    INDENT.repeat(depth)
}

fn print_op(arena: &[ReadOp], idx: usize, depth: usize, out: &mut String) {
    let op = &arena[idx];
    let pfx = indent(depth);

    match op {
        ReadOp::Source { label, bind } => {
            let lbl = label.as_ref().map(format_label_set).unwrap_or_default();
            if lbl.is_empty() {
                writeln!(out, "{pfx}Source (AS ${})", bind.0).unwrap();
            } else {
                writeln!(out, "{pfx}Source ({lbl} AS ${})", bind.0).unwrap();
            }
        }
        ReadOp::Expand {
            input,
            from,
            rel,
            to,
            bind_rel,
            bind_to,
        } => {
            writeln!(
                out,
                "{pfx}Expand (${})-[{} ${}]->({} AS ${})",
                from.0,
                format_rel_spec(rel),
                bind_rel.0,
                format_label_set(&to.labels),
                bind_to.0,
            )
            .unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Filter { input, predicate } => {
            writeln!(out, "{pfx}Filter ({})", format_expr(predicate)).unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Project { input, items } => {
            writeln!(out, "{pfx}Project [{}]", format_projections(items)).unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Aggregate { input, keys, aggs } => {
            let keys_str = keys.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            let aggs_str = aggs
                .iter()
                .map(format_agg_expr)
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{pfx}Aggregate keys=[{keys_str}] aggs=[{aggs_str}]").unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::OrderBy { input, keys } => {
            let keys_str = keys
                .iter()
                .map(format_order_key)
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{pfx}OrderBy [{keys_str}]").unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Skip { input, count } => {
            writeln!(out, "{pfx}Skip {}", format_expr(count)).unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Limit { input, count } => {
            writeln!(out, "{pfx}Limit {}", format_expr(count)).unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Distinct { input } => {
            writeln!(out, "{pfx}Distinct").unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Unwind { input, list, bind } => {
            writeln!(out, "{pfx}Unwind {} AS ${}", format_expr(list), bind.0).unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::Union { left, right, kind } => {
            let kind_str = match kind {
                UnionKind::All => "ALL",
                UnionKind::Distinct => "DISTINCT",
            };
            writeln!(out, "{pfx}Union {kind_str}").unwrap();
            print_op(arena, left.0 as usize, depth + 1, out);
            print_op(arena, right.0 as usize, depth + 1, out);
        }
        ReadOp::With {
            input,
            items,
            filter,
        } => {
            if let Some(f) = filter {
                writeln!(
                    out,
                    "{pfx}With [{}] WHERE {}",
                    format_projections(items),
                    format_expr(f)
                )
                .unwrap();
            } else {
                writeln!(out, "{pfx}With [{}]", format_projections(items)).unwrap();
            }
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::OptionalJoin { input, pattern } => {
            writeln!(out, "{pfx}OptionalJoin").unwrap();
            // Outer arm: input from arena.
            print_op(arena, input.0 as usize, depth + 1, out);
            // Inner arm: embedded tree (not in arena).
            print_op_tree(pattern, depth + 1, out);
        }
        ReadOp::ShortestPath {
            input,
            from,
            rel,
            to,
            bind_path,
            all,
        } => {
            let func = if *all {
                "allShortestPaths"
            } else {
                "shortestPath"
            };
            writeln!(
                out,
                "{pfx}ShortestPath ${} = {func}((${})-[{}]-(${}))",
                bind_path.0,
                from.0,
                format_rel_spec(rel),
                to.0,
            )
            .unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
        ReadOp::BindPath {
            input,
            bind_path,
            elements,
        } => {
            let elems = elements
                .iter()
                .map(|v| format!("${}", v.0))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{pfx}BindPath ${} = [{elems}]", bind_path.0).unwrap();
            print_op(arena, input.0 as usize, depth + 1, out);
        }
    }
}

/// Print a standalone `ReadOp` tree (used for `OptionalJoin::pattern`).
fn print_op_tree(op: &ReadOp, depth: usize, out: &mut String) {
    let pfx = indent(depth);

    match op {
        ReadOp::Source { label, bind } => {
            let lbl = label.as_ref().map(format_label_set).unwrap_or_default();
            if lbl.is_empty() {
                writeln!(out, "{pfx}Source (AS ${})", bind.0).unwrap();
            } else {
                writeln!(out, "{pfx}Source ({lbl} AS ${})", bind.0).unwrap();
            }
        }
        ReadOp::Expand {
            from,
            rel,
            to,
            bind_rel,
            bind_to,
            input: _,
        } => {
            writeln!(
                out,
                "{pfx}Expand (${})-[{} ${}]->({} AS ${})",
                from.0,
                format_rel_spec(rel),
                bind_rel.0,
                format_label_set(&to.labels),
                bind_to.0,
            )
            .unwrap();
        }
        ReadOp::Filter {
            predicate,
            input: _,
        } => {
            writeln!(out, "{pfx}Filter ({})", format_expr(predicate)).unwrap();
        }
        ReadOp::Project { items, input: _ } => {
            writeln!(out, "{pfx}Project [{}]", format_projections(items)).unwrap();
        }
        ReadOp::OptionalJoin { input: _, pattern } => {
            writeln!(out, "{pfx}OptionalJoin").unwrap();
            print_op_tree(pattern, depth + 1, out);
        }
        ReadOp::BindPath {
            bind_path,
            elements,
            input: _,
        } => {
            let elems = elements
                .iter()
                .map(|v| format!("${}", v.0))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{pfx}BindPath ${} = [{elems}]", bind_path.0).unwrap();
        }
        _ => {
            writeln!(out, "{pfx}{}", op_name(op)).unwrap();
        }
    }
}

fn op_name(op: &ReadOp) -> &'static str {
    match op {
        ReadOp::Source { .. } => "Source",
        ReadOp::Expand { .. } => "Expand",
        ReadOp::Filter { .. } => "Filter",
        ReadOp::Project { .. } => "Project",
        ReadOp::Aggregate { .. } => "Aggregate",
        ReadOp::OrderBy { .. } => "OrderBy",
        ReadOp::Skip { .. } => "Skip",
        ReadOp::Limit { .. } => "Limit",
        ReadOp::Distinct { .. } => "Distinct",
        ReadOp::Unwind { .. } => "Unwind",
        ReadOp::Union { .. } => "Union",
        ReadOp::With { .. } => "With",
        ReadOp::OptionalJoin { .. } => "OptionalJoin",
        ReadOp::ShortestPath { .. } => "ShortestPath",
        ReadOp::BindPath { .. } => "BindPath",
    }
}

fn print_write_op(op: &WriteOp, depth: usize, out: &mut String) {
    let pfx = indent(depth);
    match op {
        WriteOp::CreateNode {
            labels,
            props,
            bind,
        } => {
            let bind_str = bind.map_or(String::new(), |v| format!(" AS ${}", v.0));
            writeln!(
                out,
                "{pfx}CreateNode ({}{bind_str}) props={}",
                format_labels(labels),
                format_expr(props),
            )
            .unwrap();
        }
        WriteOp::CreateRel {
            from,
            to,
            rel_type,
            props,
            bind,
        } => {
            let bind_str = bind.map_or(String::new(), |v| format!(" AS ${}", v.0));
            writeln!(
                out,
                "{pfx}CreateRel (${})-[:{rel_type}{bind_str}]->(${})",
                from.0, to.0,
            )
            .unwrap();
            if !matches!(props, Expr::Map(v) if v.is_empty()) {
                writeln!(out, "{}  props={}", pfx, format_expr(props)).unwrap();
            }
        }
        WriteOp::MergeNode {
            labels,
            props,
            key_props,
            on_create,
            on_match,
            bind,
        } => {
            let bind_str = bind.map_or(String::new(), |v| format!(" AS ${}", v.0));
            writeln!(
                out,
                "{pfx}MergeNode ({}{bind_str}) props={}",
                format_labels(labels),
                format_expr(props),
            )
            .unwrap();
            if !key_props.is_empty() {
                writeln!(out, "{pfx}  key_props={key_props:?}").unwrap();
            }
            if !on_create.is_empty() {
                writeln!(out, "{pfx}  ON CREATE:").unwrap();
                for wop in on_create {
                    print_write_op(wop, depth + 2, out);
                }
            }
            if !on_match.is_empty() {
                writeln!(out, "{pfx}  ON MATCH:").unwrap();
                for wop in on_match {
                    print_write_op(wop, depth + 2, out);
                }
            }
        }
        WriteOp::MergeRel {
            from,
            to,
            rel_type,
            props,
            key_props,
            on_create,
            on_match,
            bind,
        } => {
            let bind_str = bind.map_or(String::new(), |v| format!(" AS ${}", v.0));
            writeln!(
                out,
                "{pfx}MergeRel (${})-[:{rel_type}{bind_str}]->(${})",
                from.0, to.0,
            )
            .unwrap();
            if !matches!(props, Expr::Map(v) if v.is_empty()) {
                writeln!(out, "{}  props={}", pfx, format_expr(props)).unwrap();
            }
            if !key_props.is_empty() {
                writeln!(out, "{pfx}  key_props={key_props:?}").unwrap();
            }
            if !on_create.is_empty() {
                writeln!(out, "{pfx}  ON CREATE:").unwrap();
                for wop in on_create {
                    print_write_op(wop, depth + 2, out);
                }
            }
            if !on_match.is_empty() {
                writeln!(out, "{pfx}  ON MATCH:").unwrap();
                for wop in on_match {
                    print_write_op(wop, depth + 2, out);
                }
            }
        }
        WriteOp::SetProperty {
            target,
            prop,
            value,
        } => {
            writeln!(
                out,
                "{pfx}SetProperty ${}.{prop} = {}",
                target.0,
                format_expr(value),
            )
            .unwrap();
        }
        WriteOp::SetLabels { target, labels } => {
            writeln!(
                out,
                "{pfx}SetLabels ${} {}",
                target.0,
                format_labels(labels),
            )
            .unwrap();
        }
        WriteOp::RemoveProperty { target, prop } => {
            writeln!(out, "{pfx}RemoveProperty ${}.{prop}", target.0).unwrap();
        }
        WriteOp::RemoveLabels { target, labels } => {
            writeln!(
                out,
                "{pfx}RemoveLabels ${} {}",
                target.0,
                format_labels(labels),
            )
            .unwrap();
        }
        WriteOp::Delete { targets, detach } => {
            let verb = if *detach { "DetachDelete" } else { "Delete" };
            let ts = targets
                .iter()
                .map(format_expr)
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{pfx}{verb} [{ts}]").unwrap();
        }
    }
}

// ── Expr formatting ───────────────────────────────────────────────────────────

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Null => "null".into(),
        Expr::Bool(b) => b.to_string(),
        Expr::Int(i) => i.to_string(),
        Expr::Float(f) => format!("{f}"),
        Expr::String(s) => format!("\"{s}\""),
        Expr::Var(v) => format!("${}", v.0),
        Expr::Prop { target, prop } => format!("{}.{prop}", format_expr(target)),
        Expr::Index { target, index } => {
            format!("{}[{}]", format_expr(target), format_expr(index))
        }
        Expr::Slice { target, start, end } => {
            let s = start.as_ref().map_or_else(String::new, |e| format_expr(e));
            let e = end.as_ref().map_or_else(String::new, |e| format_expr(e));
            format!("{}[{s}..{e}]", format_expr(target))
        }
        Expr::List(items) => {
            let s = items.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            format!("[{s}]")
        }
        Expr::Map(pairs) => {
            let s = pairs
                .iter()
                .map(|(k, v)| format!("{k}: {}", format_expr(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{s}}}")
        }
        Expr::Call { func, args } => {
            let s = args.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            format!("{func}({s})")
        }
        Expr::BinOp { op, lhs, rhs } => {
            format!(
                "({} {} {})",
                format_expr(lhs),
                format_bin_op(*op),
                format_expr(rhs)
            )
        }
        Expr::UnaryOp { op, operand } => match op {
            UnaryOp::Neg => format!("-({})", format_expr(operand)),
            UnaryOp::Not => format!("NOT({})", format_expr(operand)),
        },
        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            let scr = scrutinee
                .as_ref()
                .map(|e| format!(" {}", format_expr(e)))
                .unwrap_or_default();
            let arms_str = arms
                .iter()
                .map(|(w, t)| format!("WHEN {} THEN {}", format_expr(w), format_expr(t)))
                .collect::<Vec<_>>()
                .join(" ");
            let else_str = otherwise
                .as_ref()
                .map(|e| format!(" ELSE {}", format_expr(e)))
                .unwrap_or_default();
            format!("CASE{scr} {arms_str}{else_str} END")
        }
        Expr::IsNull { operand, negated } => {
            if *negated {
                format!("({} IS NOT NULL)", format_expr(operand))
            } else {
                format!("({} IS NULL)", format_expr(operand))
            }
        }
        Expr::InList { operand, list } => {
            format!("({} IN {})", format_expr(operand), format_expr(list))
        }
        Expr::ListPredicate {
            kind,
            var,
            iterable,
            predicate,
        } => {
            let kw = match kind {
                ListPredKind::Any => "ANY",
                ListPredKind::All => "ALL",
                ListPredKind::None => "NONE",
                ListPredKind::Single => "SINGLE",
            };
            let pred = predicate
                .as_ref()
                .map(|p| format!(" WHERE {}", format_expr(p)))
                .unwrap_or_default();
            format!("{kw}(${} IN {}{pred})", var.0, format_expr(iterable))
        }
        Expr::Param { name } => format!("${name}"),
        Expr::Exists { pattern } => {
            // Render the embedded read sub-tree inline so tooling can
            // see the pattern shape without walking into a second arena.
            // Mirrors how `OptionalJoin`'s inner `pattern` sibling is
            // printed (print_op_tree), but flattened to a single string.
            let mut inner = String::new();
            print_op_tree(pattern, 0, &mut inner);
            // Strip trailing newline so the expression renders on one
            // logical "line" (even though it may wrap).
            let inner = inner.trim_end().to_string();
            format!("EXISTS({inner})")
        }
    }
}

fn format_bin_op(op: BinOp) -> &'static str {
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
        BinOp::In => "IN",
        BinOp::StartsWith => "STARTS WITH",
        BinOp::EndsWith => "ENDS WITH",
        BinOp::Contains => "CONTAINS",
        BinOp::RegexMatch => "=~",
    }
}

fn format_projections(items: &[Projection]) -> String {
    items
        .iter()
        .map(|p| format!("{} AS {}", format_expr(&p.expr), p.alias))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_order_key(key: &OrderKey) -> String {
    let dir = match key.dir {
        SortDir::Asc => "ASC",
        SortDir::Desc => "DESC",
    };
    format!("{} {dir}", format_expr(&key.expr))
}

fn format_agg_expr(agg: &AggExpr) -> String {
    let distinct = if agg.distinct { "DISTINCT " } else { "" };
    let args = agg
        .args
        .iter()
        .map(format_expr)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({distinct}{args})", agg.func)
}

fn format_label_set(ls: &LabelSet) -> String {
    ls.0.iter()
        .map(|l| format!(":{l}"))
        .collect::<Vec<_>>()
        .concat()
}

fn format_labels(labels: &[smol_str::SmolStr]) -> String {
    labels
        .iter()
        .map(|l| format!(":{l}"))
        .collect::<Vec<_>>()
        .concat()
}

fn format_rel_spec(rel: &RelSpec) -> String {
    let types = if rel.types.is_empty() {
        String::new()
    } else {
        let joined = rel
            .types
            .iter()
            .map(smol_str::SmolStr::as_str)
            .collect::<Vec<_>>()
            .join("|");
        format!(":{joined}")
    };
    let dir = match rel.direction {
        Direction::Outgoing => "->",
        Direction::Incoming => "<-",
        Direction::Undirected => "-",
    };
    let len = match &rel.length {
        RelLength::Single => String::new(),
        RelLength::Variable { min, max } => {
            let lo = min.map_or(String::new(), |n| n.to_string());
            let hi = max.map_or(String::new(), |n| n.to_string());
            format!("*{lo}..{hi}")
        }
    };
    format!("{types}{len}({dir})")
}
