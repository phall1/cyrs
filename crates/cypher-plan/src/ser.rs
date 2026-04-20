//! Serde implementations for the plan IR (spec 0001 §12).
//!
//! All plan types derive `Serialize` + `Deserialize` behind the `serde`
//! feature gate. Enum variants use an internally-tagged representation
//! (`#[serde(tag = "op", rename_all = "snake_case")]`) so that every JSON
//! operator object carries an `"op"` discriminant field.
//!
//! Determinism: `Expr::Map` uses `Vec<(SmolStr, Expr)>` (ordered by
//! insertion, which the lowering pass controls). `PlanStatement::var_map`
//! uses `IndexMap` (insertion-ordered). No `HashMap` crosses the public API.
//! See spec §17.14.

use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{
    AggExpr, BinOp, Direction, Expr, LabelSet, NodeSpec, OpId, OrderKey, Projection, ReadOp,
    RelLength, RelSpec, SortDir, UnaryOp, UnionKind, VarId, WriteOp,
};

// ── VarId / OpId ─────────────────────────────────────────────────────────────

impl Serialize for VarId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for VarId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        u32::deserialize(d).map(VarId)
    }
}

impl Serialize for OpId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for OpId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        u32::deserialize(d).map(OpId)
    }
}

// ── LabelSet ──────────────────────────────────────────────────────────────────

impl Serialize for LabelSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for LabelSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Vec::<SmolStr>::deserialize(d).map(LabelSet)
    }
}

// ── Direction ─────────────────────────────────────────────────────────────────

impl Serialize for Direction {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let tag = match self {
            Direction::Outgoing => "outgoing",
            Direction::Incoming => "incoming",
            Direction::Undirected => "undirected",
        };
        s.serialize_str(tag)
    }
}

impl<'de> Deserialize<'de> for Direction {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "outgoing" => Ok(Direction::Outgoing),
            "incoming" => Ok(Direction::Incoming),
            "undirected" => Ok(Direction::Undirected),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["outgoing", "incoming", "undirected"],
            )),
        }
    }
}

// ── RelLength ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RelLengthSer {
    Single,
    Variable { min: Option<u64>, max: Option<u64> },
}

impl Serialize for RelLength {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let proxy = match self {
            RelLength::Single => RelLengthSer::Single,
            RelLength::Variable { min, max } => RelLengthSer::Variable {
                min: *min,
                max: *max,
            },
        };
        proxy.serialize(s)
    }
}

impl<'de> Deserialize<'de> for RelLength {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match RelLengthSer::deserialize(d)? {
            RelLengthSer::Single => Ok(RelLength::Single),
            RelLengthSer::Variable { min, max } => Ok(RelLength::Variable { min, max }),
        }
    }
}

// ── UnionKind ─────────────────────────────────────────────────────────────────

impl Serialize for UnionKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let tag = match self {
            UnionKind::All => "all",
            UnionKind::Distinct => "distinct",
        };
        s.serialize_str(tag)
    }
}

impl<'de> Deserialize<'de> for UnionKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "all" => Ok(UnionKind::All),
            "distinct" => Ok(UnionKind::Distinct),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["all", "distinct"],
            )),
        }
    }
}

// ── SortDir ───────────────────────────────────────────────────────────────────

impl Serialize for SortDir {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let tag = match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        };
        s.serialize_str(tag)
    }
}

impl<'de> Deserialize<'de> for SortDir {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            other => Err(serde::de::Error::unknown_variant(other, &["asc", "desc"])),
        }
    }
}

// ── BinOp ─────────────────────────────────────────────────────────────────────

impl Serialize for BinOp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let tag = match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Mod => "mod",
            BinOp::Pow => "pow",
            BinOp::Eq => "eq",
            BinOp::Neq => "neq",
            BinOp::Lt => "lt",
            BinOp::Le => "le",
            BinOp::Gt => "gt",
            BinOp::Ge => "ge",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Xor => "xor",
            BinOp::In => "in",
            BinOp::StartsWith => "starts_with",
            BinOp::EndsWith => "ends_with",
            BinOp::Contains => "contains",
            BinOp::RegexMatch => "regex_match",
            BinOp::Concat => "concat",
        };
        s.serialize_str(tag)
    }
}

impl<'de> Deserialize<'de> for BinOp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "add" => Ok(BinOp::Add),
            "sub" => Ok(BinOp::Sub),
            "mul" => Ok(BinOp::Mul),
            "div" => Ok(BinOp::Div),
            "mod" => Ok(BinOp::Mod),
            "pow" => Ok(BinOp::Pow),
            "eq" => Ok(BinOp::Eq),
            "neq" => Ok(BinOp::Neq),
            "lt" => Ok(BinOp::Lt),
            "le" => Ok(BinOp::Le),
            "gt" => Ok(BinOp::Gt),
            "ge" => Ok(BinOp::Ge),
            "and" => Ok(BinOp::And),
            "or" => Ok(BinOp::Or),
            "xor" => Ok(BinOp::Xor),
            "in" => Ok(BinOp::In),
            "starts_with" => Ok(BinOp::StartsWith),
            "ends_with" => Ok(BinOp::EndsWith),
            "contains" => Ok(BinOp::Contains),
            "regex_match" => Ok(BinOp::RegexMatch),
            "concat" => Ok(BinOp::Concat),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "add",
                    "sub",
                    "mul",
                    "div",
                    "mod",
                    "pow",
                    "eq",
                    "neq",
                    "lt",
                    "le",
                    "gt",
                    "ge",
                    "and",
                    "or",
                    "xor",
                    "in",
                    "starts_with",
                    "ends_with",
                    "contains",
                    "regex_match",
                    "concat",
                ],
            )),
        }
    }
}

// ── UnaryOp ───────────────────────────────────────────────────────────────────

impl Serialize for UnaryOp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let tag = match self {
            UnaryOp::Neg => "neg",
            UnaryOp::Not => "not",
        };
        s.serialize_str(tag)
    }
}

impl<'de> Deserialize<'de> for UnaryOp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "neg" => Ok(UnaryOp::Neg),
            "not" => Ok(UnaryOp::Not),
            other => Err(serde::de::Error::unknown_variant(other, &["neg", "not"])),
        }
    }
}

// ── NodeSpec / RelSpec ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeSpecSer {
    labels: LabelSet,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<Expr>,
}

impl Serialize for NodeSpec {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        NodeSpecSer {
            labels: self.labels.clone(),
            properties: self.properties.clone(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for NodeSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = NodeSpecSer::deserialize(d)?;
        Ok(NodeSpec {
            labels: proxy.labels,
            properties: proxy.properties,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelSpecSer {
    types: Vec<SmolStr>,
    direction: Direction,
    length: RelLength,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<Expr>,
}

impl Serialize for RelSpec {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RelSpecSer {
            types: self.types.clone(),
            direction: self.direction,
            length: self.length.clone(),
            properties: self.properties.clone(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for RelSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = RelSpecSer::deserialize(d)?;
        Ok(RelSpec {
            types: proxy.types,
            direction: proxy.direction,
            length: proxy.length,
            properties: proxy.properties,
        })
    }
}

// ── Projection / OrderKey / AggExpr ──────────────────────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionSer {
    expr: Expr,
    alias: SmolStr,
}

impl Serialize for Projection {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProjectionSer {
            expr: self.expr.clone(),
            alias: self.alias.clone(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Projection {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = ProjectionSer::deserialize(d)?;
        Ok(Projection {
            expr: proxy.expr,
            alias: proxy.alias,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderKeySer {
    expr: Expr,
    dir: SortDir,
}

impl Serialize for OrderKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        OrderKeySer {
            expr: self.expr.clone(),
            dir: self.dir,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for OrderKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = OrderKeySer::deserialize(d)?;
        Ok(OrderKey {
            expr: proxy.expr,
            dir: proxy.dir,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AggExprSer {
    func: SmolStr,
    args: Vec<Expr>,
    distinct: bool,
}

impl Serialize for AggExpr {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AggExprSer {
            func: self.func.clone(),
            args: self.args.clone(),
            distinct: self.distinct,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for AggExpr {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = AggExprSer::deserialize(d)?;
        Ok(AggExpr {
            func: proxy.func,
            args: proxy.args,
            distinct: proxy.distinct,
        })
    }
}

// ── Expr ──────────────────────────────────────────────────────────────────────
//
// Internally tagged via `"kind"` discriminant (not `"op"` — that would
// conflict with the `op` field in BinOp/UnaryOp variants). Each variant
// carries only the fields the spec lists.

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExprSer {
    Null,
    Bool {
        value: bool,
    },
    Int {
        value: i64,
    },
    Float {
        value: f64,
    },
    #[serde(rename = "str")]
    String {
        value: SmolStr,
    },
    Var {
        id: VarId,
    },
    Prop {
        target: Box<Expr>,
        prop: SmolStr,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    List {
        items: Vec<Expr>,
    },
    Map {
        // Vec preserves insertion order (deterministic). Spec §17.14.
        entries: Vec<(SmolStr, Expr)>,
    },
    Call {
        func: SmolStr,
        args: Vec<Expr>,
    },
    BinOp {
        bin_op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    UnaryOp {
        unary_op: UnaryOp,
        operand: Box<Expr>,
    },
    Case {
        #[serde(skip_serializing_if = "Option::is_none")]
        scrutinee: Option<Box<Expr>>,
        arms: Vec<(Expr, Expr)>,
        #[serde(skip_serializing_if = "Option::is_none")]
        otherwise: Option<Box<Expr>>,
    },
    IsNull {
        operand: Box<Expr>,
        negated: bool,
    },
    InList {
        operand: Box<Expr>,
        list: Box<Expr>,
    },
    Param {
        name: SmolStr,
    },
}

impl Serialize for Expr {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let proxy: ExprSer = match self {
            Expr::Null => ExprSer::Null,
            Expr::Bool(v) => ExprSer::Bool { value: *v },
            Expr::Int(v) => ExprSer::Int { value: *v },
            Expr::Float(v) => ExprSer::Float { value: *v },
            Expr::String(v) => ExprSer::String { value: v.clone() },
            Expr::Var(id) => ExprSer::Var { id: *id },
            Expr::Prop { target, prop } => ExprSer::Prop {
                target: Box::new(*target.clone()),
                prop: prop.clone(),
            },
            Expr::Index { target, index } => ExprSer::Index {
                target: Box::new(*target.clone()),
                index: Box::new(*index.clone()),
            },
            Expr::List(items) => ExprSer::List {
                items: items.clone(),
            },
            Expr::Map(entries) => ExprSer::Map {
                entries: entries.clone(),
            },
            Expr::Call { func, args } => ExprSer::Call {
                func: func.clone(),
                args: args.clone(),
            },
            Expr::BinOp { op, lhs, rhs } => ExprSer::BinOp {
                bin_op: *op,
                lhs: Box::new(*lhs.clone()),
                rhs: Box::new(*rhs.clone()),
            },
            Expr::UnaryOp { op, operand } => ExprSer::UnaryOp {
                unary_op: *op,
                operand: Box::new(*operand.clone()),
            },
            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => ExprSer::Case {
                scrutinee: scrutinee.as_ref().map(|e| Box::new(*e.clone())),
                arms: arms.clone(),
                otherwise: otherwise.as_ref().map(|e| Box::new(*e.clone())),
            },
            Expr::IsNull { operand, negated } => ExprSer::IsNull {
                operand: Box::new(*operand.clone()),
                negated: *negated,
            },
            Expr::InList { operand, list } => ExprSer::InList {
                operand: Box::new(*operand.clone()),
                list: Box::new(*list.clone()),
            },
            Expr::Param { name } => ExprSer::Param { name: name.clone() },
        };
        proxy.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = ExprSer::deserialize(d)?;
        Ok(match proxy {
            ExprSer::Null => Expr::Null,
            ExprSer::Bool { value } => Expr::Bool(value),
            ExprSer::Int { value } => Expr::Int(value),
            ExprSer::Float { value } => Expr::Float(value),
            ExprSer::String { value } => Expr::String(value),
            ExprSer::Var { id } => Expr::Var(id),
            ExprSer::Prop { target, prop } => Expr::Prop {
                target: Box::new(*target),
                prop,
            },
            ExprSer::Index { target, index } => Expr::Index {
                target: Box::new(*target),
                index: Box::new(*index),
            },
            ExprSer::List { items } => Expr::List(items),
            ExprSer::Map { entries } => Expr::Map(entries),
            ExprSer::Call { func, args } => Expr::Call { func, args },
            ExprSer::BinOp { bin_op, lhs, rhs } => Expr::BinOp {
                op: bin_op,
                lhs: Box::new(*lhs),
                rhs: Box::new(*rhs),
            },
            ExprSer::UnaryOp { unary_op, operand } => Expr::UnaryOp {
                op: unary_op,
                operand: Box::new(*operand),
            },
            ExprSer::Case {
                scrutinee,
                arms,
                otherwise,
            } => Expr::Case {
                scrutinee: scrutinee.map(|e| Box::new(*e)),
                arms,
                otherwise: otherwise.map(|e| Box::new(*e)),
            },
            ExprSer::IsNull { operand, negated } => Expr::IsNull {
                operand: Box::new(*operand),
                negated,
            },
            ExprSer::InList { operand, list } => Expr::InList {
                operand: Box::new(*operand),
                list: Box::new(*list),
            },
            ExprSer::Param { name } => Expr::Param { name },
        })
    }
}

// ── ReadOp ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ReadOpSer {
    Source {
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<LabelSet>,
        bind: VarId,
    },
    Expand {
        input: OpId,
        from: VarId,
        rel: RelSpec,
        to: NodeSpec,
        bind_rel: VarId,
        bind_to: VarId,
    },
    Filter {
        input: OpId,
        predicate: Expr,
    },
    Project {
        input: OpId,
        items: Vec<Projection>,
    },
    Aggregate {
        input: OpId,
        keys: Vec<Expr>,
        aggs: Vec<AggExpr>,
    },
    OrderBy {
        input: OpId,
        keys: Vec<OrderKey>,
    },
    Skip {
        input: OpId,
        count: Expr,
    },
    Limit {
        input: OpId,
        count: Expr,
    },
    Distinct {
        input: OpId,
    },
    Unwind {
        input: OpId,
        list: Expr,
        bind: VarId,
    },
    Union {
        left: OpId,
        right: OpId,
        kind: UnionKind,
    },
    With {
        input: OpId,
        items: Vec<Projection>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filter: Option<Expr>,
    },
    OptionalJoin {
        input: OpId,
        pattern: Box<ReadOp>,
    },
}

impl Serialize for ReadOp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let proxy: ReadOpSer = match self {
            ReadOp::Source { label, bind } => ReadOpSer::Source {
                label: label.clone(),
                bind: *bind,
            },
            ReadOp::Expand {
                input,
                from,
                rel,
                to,
                bind_rel,
                bind_to,
            } => ReadOpSer::Expand {
                input: *input,
                from: *from,
                rel: rel.clone(),
                to: to.clone(),
                bind_rel: *bind_rel,
                bind_to: *bind_to,
            },
            ReadOp::Filter { input, predicate } => ReadOpSer::Filter {
                input: *input,
                predicate: predicate.clone(),
            },
            ReadOp::Project { input, items } => ReadOpSer::Project {
                input: *input,
                items: items.clone(),
            },
            ReadOp::Aggregate { input, keys, aggs } => ReadOpSer::Aggregate {
                input: *input,
                keys: keys.clone(),
                aggs: aggs.clone(),
            },
            ReadOp::OrderBy { input, keys } => ReadOpSer::OrderBy {
                input: *input,
                keys: keys.clone(),
            },
            ReadOp::Skip { input, count } => ReadOpSer::Skip {
                input: *input,
                count: count.clone(),
            },
            ReadOp::Limit { input, count } => ReadOpSer::Limit {
                input: *input,
                count: count.clone(),
            },
            ReadOp::Distinct { input } => ReadOpSer::Distinct { input: *input },
            ReadOp::Unwind { input, list, bind } => ReadOpSer::Unwind {
                input: *input,
                list: list.clone(),
                bind: *bind,
            },
            ReadOp::Union { left, right, kind } => ReadOpSer::Union {
                left: *left,
                right: *right,
                kind: *kind,
            },
            ReadOp::With {
                input,
                items,
                filter,
            } => ReadOpSer::With {
                input: *input,
                items: items.clone(),
                filter: filter.clone(),
            },
            ReadOp::OptionalJoin { input, pattern } => ReadOpSer::OptionalJoin {
                input: *input,
                pattern: pattern.clone(),
            },
        };
        proxy.serialize(s)
    }
}

impl<'de> Deserialize<'de> for ReadOp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = ReadOpSer::deserialize(d)?;
        Ok(match proxy {
            ReadOpSer::Source { label, bind } => ReadOp::Source { label, bind },
            ReadOpSer::Expand {
                input,
                from,
                rel,
                to,
                bind_rel,
                bind_to,
            } => ReadOp::Expand {
                input,
                from,
                rel,
                to,
                bind_rel,
                bind_to,
            },
            ReadOpSer::Filter { input, predicate } => ReadOp::Filter { input, predicate },
            ReadOpSer::Project { input, items } => ReadOp::Project { input, items },
            ReadOpSer::Aggregate { input, keys, aggs } => ReadOp::Aggregate { input, keys, aggs },
            ReadOpSer::OrderBy { input, keys } => ReadOp::OrderBy { input, keys },
            ReadOpSer::Skip { input, count } => ReadOp::Skip { input, count },
            ReadOpSer::Limit { input, count } => ReadOp::Limit { input, count },
            ReadOpSer::Distinct { input } => ReadOp::Distinct { input },
            ReadOpSer::Unwind { input, list, bind } => ReadOp::Unwind { input, list, bind },
            ReadOpSer::Union { left, right, kind } => ReadOp::Union { left, right, kind },
            ReadOpSer::With {
                input,
                items,
                filter,
            } => ReadOp::With {
                input,
                items,
                filter,
            },
            ReadOpSer::OptionalJoin { input, pattern } => ReadOp::OptionalJoin { input, pattern },
        })
    }
}

// ── WriteOp ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum WriteOpSer {
    CreateNode {
        labels: Vec<SmolStr>,
        props: Expr,
        #[serde(skip_serializing_if = "Option::is_none")]
        bind: Option<VarId>,
    },
    CreateRel {
        from: VarId,
        to: VarId,
        rel_type: SmolStr,
        props: Expr,
        #[serde(skip_serializing_if = "Option::is_none")]
        bind: Option<VarId>,
    },
    MergeNode {
        labels: Vec<SmolStr>,
        props: Expr,
        on_create: Vec<WriteOp>,
        on_match: Vec<WriteOp>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bind: Option<VarId>,
    },
    MergeRel {
        from: VarId,
        to: VarId,
        rel_type: SmolStr,
        props: Expr,
        on_create: Vec<WriteOp>,
        on_match: Vec<WriteOp>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bind: Option<VarId>,
    },
    SetProperty {
        target: VarId,
        prop: SmolStr,
        value: Expr,
    },
    SetLabels {
        target: VarId,
        labels: Vec<SmolStr>,
    },
    RemoveProperty {
        target: VarId,
        prop: SmolStr,
    },
    RemoveLabels {
        target: VarId,
        labels: Vec<SmolStr>,
    },
    Delete {
        targets: Vec<Expr>,
        detach: bool,
    },
}

impl Serialize for WriteOp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let proxy: WriteOpSer = match self {
            WriteOp::CreateNode {
                labels,
                props,
                bind,
            } => WriteOpSer::CreateNode {
                labels: labels.clone(),
                props: props.clone(),
                bind: *bind,
            },
            WriteOp::CreateRel {
                from,
                to,
                rel_type,
                props,
                bind,
            } => WriteOpSer::CreateRel {
                from: *from,
                to: *to,
                rel_type: rel_type.clone(),
                props: props.clone(),
                bind: *bind,
            },
            WriteOp::MergeNode {
                labels,
                props,
                on_create,
                on_match,
                bind,
            } => WriteOpSer::MergeNode {
                labels: labels.clone(),
                props: props.clone(),
                on_create: on_create.clone(),
                on_match: on_match.clone(),
                bind: *bind,
            },
            WriteOp::MergeRel {
                from,
                to,
                rel_type,
                props,
                on_create,
                on_match,
                bind,
            } => WriteOpSer::MergeRel {
                from: *from,
                to: *to,
                rel_type: rel_type.clone(),
                props: props.clone(),
                on_create: on_create.clone(),
                on_match: on_match.clone(),
                bind: *bind,
            },
            WriteOp::SetProperty {
                target,
                prop,
                value,
            } => WriteOpSer::SetProperty {
                target: *target,
                prop: prop.clone(),
                value: value.clone(),
            },
            WriteOp::SetLabels { target, labels } => WriteOpSer::SetLabels {
                target: *target,
                labels: labels.clone(),
            },
            WriteOp::RemoveProperty { target, prop } => WriteOpSer::RemoveProperty {
                target: *target,
                prop: prop.clone(),
            },
            WriteOp::RemoveLabels { target, labels } => WriteOpSer::RemoveLabels {
                target: *target,
                labels: labels.clone(),
            },
            WriteOp::Delete { targets, detach } => WriteOpSer::Delete {
                targets: targets.clone(),
                detach: *detach,
            },
        };
        proxy.serialize(s)
    }
}

impl<'de> Deserialize<'de> for WriteOp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = WriteOpSer::deserialize(d)?;
        Ok(match proxy {
            WriteOpSer::CreateNode {
                labels,
                props,
                bind,
            } => WriteOp::CreateNode {
                labels,
                props,
                bind,
            },
            WriteOpSer::CreateRel {
                from,
                to,
                rel_type,
                props,
                bind,
            } => WriteOp::CreateRel {
                from,
                to,
                rel_type,
                props,
                bind,
            },
            WriteOpSer::MergeNode {
                labels,
                props,
                on_create,
                on_match,
                bind,
            } => WriteOp::MergeNode {
                labels,
                props,
                on_create,
                on_match,
                bind,
            },
            WriteOpSer::MergeRel {
                from,
                to,
                rel_type,
                props,
                on_create,
                on_match,
                bind,
            } => WriteOp::MergeRel {
                from,
                to,
                rel_type,
                props,
                on_create,
                on_match,
                bind,
            },
            WriteOpSer::SetProperty {
                target,
                prop,
                value,
            } => WriteOp::SetProperty {
                target,
                prop,
                value,
            },
            WriteOpSer::SetLabels { target, labels } => WriteOp::SetLabels { target, labels },
            WriteOpSer::RemoveProperty { target, prop } => WriteOp::RemoveProperty { target, prop },
            WriteOpSer::RemoveLabels { target, labels } => WriteOp::RemoveLabels { target, labels },
            WriteOpSer::Delete { targets, detach } => WriteOp::Delete { targets, detach },
        })
    }
}

// ── PlanStatement ─────────────────────────────────────────────────────────────
//
// `var_map` is IndexMap<VarId, HirVarId>. Serialised as an array of
// `[plan_var_u32, hir_var_u32]` pairs to keep ordering stable.

use crate::lower::PlanStatement;
use cypher_hir::VarId as HirVarId;
use indexmap::IndexMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanStatementSer {
    ops: Vec<ReadOp>,
    write_ops: Vec<WriteOp>,
    /// Ordered pairs of `(plan_var_id, hir_var_id)`. Insertion order is
    /// preserved by `IndexMap` (spec §17.14 determinism).
    var_map: Vec<(u32, u32)>,
}

impl Serialize for PlanStatement {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let var_map: Vec<(u32, u32)> = self
            .var_map
            .iter()
            .map(|(plan_v, hir_v)| (plan_v.0, hir_v.0))
            .collect();
        PlanStatementSer {
            ops: self.ops.clone(),
            write_ops: self.write_ops.clone(),
            var_map,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for PlanStatement {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = PlanStatementSer::deserialize(d)?;
        let mut var_map: IndexMap<VarId, HirVarId> = IndexMap::with_capacity(proxy.var_map.len());
        for (plan_v, hir_v) in proxy.var_map {
            var_map.insert(VarId(plan_v), HirVarId(hir_v));
        }
        Ok(PlanStatement {
            ops: proxy.ops,
            write_ops: proxy.write_ops,
            var_map,
        })
    }
}
