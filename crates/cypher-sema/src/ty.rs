//! Cypher value-level type system (spec 0001 §7.2).
//!
//! # `Type` vs `VarKind`
//!
//! `VarKind` (from `cypher-hir`) is a **structural binder kind**: it
//! records what syntactic construct *produced* a variable — a node pattern,
//! a relationship pattern, a path assignment, or a general value binder.
//! `VarKind` is set once, at binding time, and never changes.
//!
//! [`Type`] is the **computed value type** of an expression: what kind of
//! Cypher value flows through that expression at query evaluation time.
//! Types are inferred by the type-inference pass (cy-c6g) using a small
//! unification engine. A node variable has `VarKind::Node` and `Type::Node(_)`,
//! but a function call returning a boolean has `VarKind::Value` and
//! `Type::Bool`. Do not conflate the two.
//!
//! # Type lattice
//!
//! ```text
//!   Any
//!    ├─ Null
//!    ├─ Bool
//!    ├─ Num ─── Int
//!    │      └── Float
//!    ├─ String
//!    ├─ Date
//!    ├─ Datetime
//!    ├─ List(T)
//!    ├─ Map(fields)
//!    ├─ Node(labels?)
//!    ├─ Relationship(type?)
//!    ├─ Path
//!    └─ Union(T₁ | T₂ | …)
//!   Unknown  (inference failure / "bottom from above")
//! ```
//!
//! `Any` is the **universal super-type**: schema-free queries yield
//! `Any`-typed property reads, and `Any` unifies with everything without
//! error. `Unknown` is the inference-failure sentinel — produced when
//! conflicting constraints cannot be resolved — and propagates through
//! subsequent inference steps silently so we emit at most one error per
//! root cause.
//!
//! `Num` is a numeric super-type covering `Int ∪ Float` (spec §7.2).
//! It is used during arithmetic unification: `Int + Float` widens to `Num`.
//! Division of `Int / Int` stays `Int` (openCypher semantics, no automatic
//! promotion).
//!
//! `Union` represents the join of two or more types at a branching point
//! (e.g., `CASE` arms, or `RETURN` over alternation branches). The
//! contained `Vec<Type>` is always **sorted and de-duplicated** so that two
//! structurally equal unions have identical byte representations. Use
//! [`Type::union`] to construct canonicalised unions.

use smol_str::SmolStr;
use std::collections::BTreeMap;

/// Cypher value-level type. Spec §7.2.
///
/// See the [module-level documentation](self) for the distinction between
/// `Type` (inferred, expression-level) and `VarKind` (structural, binding-level).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    /// Universal super-type. Property reads without schema are `Any`.
    Any,
    /// The `null` literal type.
    Null,
    /// Boolean.
    Bool,
    /// 64-bit integer.
    Int,
    /// 64-bit IEEE-754 float.
    Float,
    /// Numeric super-type: `Int ∨ Float`. Used during arithmetic unification.
    Num,
    /// UTF-8 string.
    String,
    /// Calendar date (ISO 8601).
    Date,
    /// Date-time (ISO 8601 with time zone). v1 temporal cap (spec §19).
    Datetime,
    /// Homogeneous list; element type is boxed to avoid infinite size.
    List(Box<Type>),
    /// Structural map. When field types are known (schema-aware mode), the
    /// map carries a `BTreeMap<SmolStr, Type>` so that field order is stable
    /// and equality / hashing are deterministic. An empty map means "any map".
    Map(BTreeMap<SmolStr, Type>),
    /// A graph node, optionally constrained to a set of labels.
    Node(Option<LabelSet>),
    /// A graph relationship, optionally constrained to a single type name.
    Relationship(Option<SmolStr>),
    /// A graph path (sequence of alternating nodes and relationships).
    Path,
    /// Sum type for alternation branches. Always sorted and de-duplicated.
    /// Use [`Type::union`] to construct; never build the variant directly.
    Union(Vec<Type>),
    /// Inference-failure sentinel. Propagates silently to suppress cascades.
    Unknown,
}

/// An ordered, de-duplicated set of label names.
///
/// Labels are stored in a `Vec<SmolStr>` sorted lexicographically so that
/// equality is structural and `Hash` is deterministic (spec §17.14).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LabelSet(pub Vec<SmolStr>);

impl LabelSet {
    /// Construct a `LabelSet` from an iterator, sorting and de-duplicating.
    pub fn new(labels: impl IntoIterator<Item = SmolStr>) -> Self {
        let mut v: Vec<SmolStr> = labels.into_iter().collect();
        v.sort_unstable();
        v.dedup();
        LabelSet(v)
    }
}

impl Type {
    // -----------------------------------------------------------------------
    // Convenience constructors
    // -----------------------------------------------------------------------

    /// `Num` — the numeric super-type (`Int ∨ Float`).
    #[inline]
    pub fn num() -> Self {
        Type::Num
    }

    /// Wrap a type as nullable: `T → Union([T, Null])`.
    ///
    /// If `inner` is already `Null` or `Any`, returns it unchanged (both
    /// already admit `null`). If `inner` is `Unknown`, returns `Unknown`.
    pub fn nullable(inner: Type) -> Self {
        match &inner {
            Type::Null | Type::Any | Type::Unknown => inner,
            Type::Union(members) if members.contains(&Type::Null) => inner,
            _ => Type::union([inner, Type::Null]),
        }
    }

    /// Construct a canonicalised `Union` from an iterator of types.
    ///
    /// Canonicalisation rules:
    /// 1. Flatten nested `Union`s.
    /// 2. Remove duplicates.
    /// 3. Sort by the derived `Ord` on `Type` for a stable representation.
    /// 4. If after flattening the set has zero members, return `Unknown`.
    /// 5. If it has exactly one member, return that member directly (no
    ///    singleton `Union`).
    pub fn union(types: impl IntoIterator<Item = Type>) -> Self {
        let mut members: Vec<Type> = Vec::new();
        for t in types {
            match t {
                Type::Union(inner) => members.extend(inner),
                other => members.push(other),
            }
        }
        // De-duplicate (requires sort first so consecutive dupes collapse).
        members.sort();
        members.dedup();

        match members.len() {
            0 => Type::Unknown,
            1 => members.remove(0),
            _ => Type::Union(members),
        }
    }

    /// Return `true` if this type admits the `null` value.
    pub fn is_nullable(&self) -> bool {
        match self {
            Type::Null | Type::Any | Type::Unknown => true,
            Type::Union(members) => members.contains(&Type::Null),
            _ => false,
        }
    }

    /// Return `true` if this is the `Any` super-type.
    #[inline]
    pub fn is_any(&self) -> bool {
        matches!(self, Type::Any)
    }

    /// Return `true` if this is the `Unknown` sentinel.
    #[inline]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }

    /// Return `true` if this type is numeric (`Int`, `Float`, or `Num`).
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float | Type::Num)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Equality and hash
    // -----------------------------------------------------------------------

    #[test]
    fn type_equality_same_variant() {
        assert_eq!(Type::Bool, Type::Bool);
        assert_eq!(Type::Int, Type::Int);
        assert_eq!(Type::Any, Type::Any);
        assert_eq!(Type::Unknown, Type::Unknown);
    }

    #[test]
    fn type_equality_different_variants() {
        assert_ne!(Type::Int, Type::Float);
        assert_ne!(Type::Any, Type::Null);
        assert_ne!(Type::Bool, Type::Unknown);
    }

    #[test]
    fn type_list_equality() {
        let a = Type::List(Box::new(Type::Int));
        let b = Type::List(Box::new(Type::Int));
        let c = Type::List(Box::new(Type::Float));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn type_map_equality() {
        let mut m1 = BTreeMap::new();
        m1.insert(SmolStr::new("x"), Type::Int);
        let mut m2 = BTreeMap::new();
        m2.insert(SmolStr::new("x"), Type::Int);
        assert_eq!(Type::Map(m1), Type::Map(m2));
    }

    #[test]
    fn type_node_equality() {
        let a = Type::Node(None);
        let b = Type::Node(None);
        assert_eq!(a, b);

        let c = Type::Node(Some(LabelSet::new([SmolStr::new("Person")])));
        let d = Type::Node(Some(LabelSet::new([SmolStr::new("Person")])));
        assert_eq!(c, d);
        assert_ne!(a, c);
    }

    // -----------------------------------------------------------------------
    // LabelSet canonicalisation
    // -----------------------------------------------------------------------

    #[test]
    fn label_set_dedup_and_sort() {
        let ls = LabelSet::new([
            SmolStr::new("Zebra"),
            SmolStr::new("Apple"),
            SmolStr::new("Apple"),
        ]);
        assert_eq!(ls.0, vec![SmolStr::new("Apple"), SmolStr::new("Zebra")]);
    }

    // -----------------------------------------------------------------------
    // nullable roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn nullable_wraps_non_null_type() {
        let t = Type::nullable(Type::Int);
        // Should be Union([Int, Null]) in canonical order.
        // Ord: Null < Int (they appear in enum order, Null before Int).
        assert_eq!(t, Type::union([Type::Int, Type::Null]));
    }

    #[test]
    fn nullable_idempotent_on_null() {
        assert_eq!(Type::nullable(Type::Null), Type::Null);
    }

    #[test]
    fn nullable_idempotent_on_any() {
        assert_eq!(Type::nullable(Type::Any), Type::Any);
    }

    #[test]
    fn nullable_idempotent_on_unknown() {
        assert_eq!(Type::nullable(Type::Unknown), Type::Unknown);
    }

    #[test]
    fn nullable_idempotent_when_already_nullable_union() {
        let already = Type::union([Type::Int, Type::Null]);
        let double = Type::nullable(already.clone());
        assert_eq!(already, double);
    }

    // -----------------------------------------------------------------------
    // Union canonicalisation
    // -----------------------------------------------------------------------

    #[test]
    fn union_order_insensitive() {
        let a = Type::union([Type::Int, Type::Bool]);
        let b = Type::union([Type::Bool, Type::Int]);
        assert_eq!(a, b, "Union must be order-insensitive");
    }

    #[test]
    fn union_deduplicates() {
        let t = Type::union([Type::Int, Type::Int, Type::Bool]);
        let expected = Type::union([Type::Int, Type::Bool]);
        assert_eq!(t, expected);
    }

    #[test]
    fn union_flattens_nested_unions() {
        let inner = Type::union([Type::Int, Type::Bool]);
        let outer = Type::union([inner, Type::String]);
        let flat = Type::union([Type::Int, Type::Bool, Type::String]);
        assert_eq!(outer, flat);
    }

    #[test]
    fn union_singleton_unwraps() {
        let t = Type::union([Type::Int]);
        assert_eq!(t, Type::Int);
    }

    #[test]
    fn union_empty_is_unknown() {
        let t = Type::union(std::iter::empty());
        assert_eq!(t, Type::Unknown);
    }

    // -----------------------------------------------------------------------
    // Any / Unknown semantics
    // -----------------------------------------------------------------------

    #[test]
    fn any_is_not_unknown() {
        assert_ne!(Type::Any, Type::Unknown);
        assert!(Type::Any.is_any());
        assert!(!Type::Any.is_unknown());
    }

    #[test]
    fn unknown_is_not_any() {
        assert!(Type::Unknown.is_unknown());
        assert!(!Type::Unknown.is_any());
    }

    #[test]
    fn any_is_nullable() {
        assert!(Type::Any.is_nullable());
    }

    #[test]
    fn null_is_nullable() {
        assert!(Type::Null.is_nullable());
    }

    #[test]
    fn bool_is_not_nullable() {
        assert!(!Type::Bool.is_nullable());
    }

    // -----------------------------------------------------------------------
    // Num super-type
    // -----------------------------------------------------------------------

    #[test]
    fn num_is_distinct_variant() {
        assert_ne!(Type::Num, Type::Int);
        assert_ne!(Type::Num, Type::Float);
        assert!(Type::num().is_numeric());
        assert!(Type::Int.is_numeric());
        assert!(Type::Float.is_numeric());
        assert!(!Type::Bool.is_numeric());
    }

    // -----------------------------------------------------------------------
    // Hash is deterministic for BTreeMap-backed Map
    // -----------------------------------------------------------------------

    #[test]
    fn map_type_hash_is_stable() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_it(t: &Type) -> u64 {
            let mut h = DefaultHasher::new();
            t.hash(&mut h);
            h.finish()
        }

        let mut m1 = BTreeMap::new();
        m1.insert(SmolStr::new("a"), Type::Int);
        m1.insert(SmolStr::new("b"), Type::Bool);

        let mut m2 = BTreeMap::new();
        // Insert in reverse order — BTreeMap normalises, so hash must match.
        m2.insert(SmolStr::new("b"), Type::Bool);
        m2.insert(SmolStr::new("a"), Type::Int);

        assert_eq!(
            hash_it(&Type::Map(m1)),
            hash_it(&Type::Map(m2)),
            "Map hash must be insertion-order-independent"
        );
    }

    // -----------------------------------------------------------------------
    // Union hash is deterministic
    // -----------------------------------------------------------------------

    #[test]
    fn union_hash_is_stable() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_it(t: &Type) -> u64 {
            let mut h = DefaultHasher::new();
            t.hash(&mut h);
            h.finish()
        }

        let a = Type::union([Type::Int, Type::Bool]);
        let b = Type::union([Type::Bool, Type::Int]);
        assert_eq!(
            hash_it(&a),
            hash_it(&b),
            "Union hash must be order-insensitive"
        );
    }
}
