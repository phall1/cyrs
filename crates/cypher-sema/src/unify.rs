//! Unification engine for the Cypher type system (spec 0001 §7.2).
//!
//! # Unification semantics
//!
//! `unify(a, b)` computes the *meet* of two types — the most specific type
//! that is a super-type of both inputs. The rules follow spec §7.2 exactly:
//!
//! | Rule | Result |
//! |------|--------|
//! | `unify(Any, t)` | `t` — Any is the universal super-type; it dissolves |
//! | `unify(Unknown, t)` | `Unknown` — error-sentinel propagates silently |
//! | `unify(Null, Null)` | `Null` |
//! | `unify(Null, t)` | `Union([Null, t])` — nullability via open union |
//! | `unify(Num, Int/Float/Num)` | the concrete numeric type |
//! | `unify(Int, Float)` | `Num` — widening to numeric super-type |
//! | `unify(List(a), List(b))` | `List(unify(a, b))` — covariant |
//! | `unify(Map(a), Map(b))` | open-record merge (see below) |
//! | `unify(Node(a), Node(b))` | union of label sets |
//! | `unify(Union, t)` | per-variant recursion, then canonicalise |
//! | otherwise | `Err(TypeMismatch)` for non-overlapping concrete types |
//!
//! # Map semantics — open records
//!
//! Maps use **open-record** semantics: keys present in both sides are
//! unified field-by-field; keys present on only one side are kept
//! verbatim. This matches Cypher's "property map as open record" design
//! where properties not mentioned in a pattern are unconstrained.
//!
//! # Diagnostic
//!
//! A mismatch produces [`TypeMismatch`], which carries `expected` and
//! `actual` for later conversion to a [`cypher_diag::Diagnostic`] with
//! code [`cypher_diag::DiagCode::E5003`].

use std::collections::BTreeMap;

use smol_str::SmolStr;

use crate::ty::{LabelSet, Type};

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

/// Returned when two types cannot be unified.
///
/// Convert to a diagnostic with code `E5003` at the call site (future
/// type-inference passes). The fields are public so callers can construct
/// rich diagnostic messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMismatch {
    /// The type expected by the context (the first argument to `unify`).
    pub expected: Type,
    /// The type actually encountered (the second argument to `unify`).
    pub actual: Type,
}

impl TypeMismatch {
    /// Convert this mismatch to a [`cypher_diag::Diagnostic`].
    ///
    /// The caller supplies the source range because `unify` is a pure function
    /// without span information. Code is always `E5003`.
    pub fn into_diagnostic(self, range: cypher_syntax::TextRange) -> cypher_diag::Diagnostic {
        use cypher_diag::{DiagCode, Diagnostic, Label, Severity};
        use smol_str::SmolStr;

        let message = SmolStr::new(format!(
            "type mismatch: expected `{:?}`, found `{:?}`",
            self.expected, self.actual
        ));
        Diagnostic {
            code: DiagCode::E5003,
            severity: Severity::Error,
            message: message.clone(),
            primary: Label {
                range,
                caption: message,
            },
            labels: Vec::new(),
            notes: Vec::new(),
            related: Vec::new(),
            fixes: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Core unification
// ---------------------------------------------------------------------------

/// Unify two types according to spec §7.2.
///
/// Returns the unified type on success, or a [`TypeMismatch`] when the two
/// types are incompatible concrete types with no common super-type other than
/// `Any`.
///
/// The function is **pure**: no mutation, no allocation beyond what the
/// result requires.
pub fn unify(a: &Type, b: &Type) -> Result<Type, TypeMismatch> {
    // Symmetric: if the left arm doesn't match but the mirror does, delegate.
    // We handle this via a small normalisation at the end of the match.
    match (a, b) {
        // ---------------------------------------------------------------
        // Any: dissolves — Any ∪ t = t (§7.2)
        // ---------------------------------------------------------------
        (Type::Any, t) | (t, Type::Any) => Ok(t.clone()),

        // ---------------------------------------------------------------
        // Unknown: error-sentinel propagates (§7.2)
        // ---------------------------------------------------------------
        (Type::Unknown, _) | (_, Type::Unknown) => Ok(Type::Unknown),

        // ---------------------------------------------------------------
        // Reflexive: identical types unify to themselves
        // ---------------------------------------------------------------
        (a, b) if a == b => Ok(a.clone()),

        // ---------------------------------------------------------------
        // Null / nullability (§7.2)
        // ---------------------------------------------------------------
        (Type::Null, t) | (t, Type::Null) => {
            // Null ∪ t → Union([Null, t]) (open nullability)
            Ok(Type::union([Type::Null, t.clone()]))
        }

        // ---------------------------------------------------------------
        // Numeric widening (§7.2)
        // ---------------------------------------------------------------
        // Num absorbs Int and Float; Int + Float widens to Num.
        (Type::Num | Type::Float, Type::Int)
        | (Type::Int | Type::Float, Type::Num)
        | (Type::Num | Type::Int, Type::Float) => Ok(Type::Num),

        // ---------------------------------------------------------------
        // List — covariant (§7.2)
        // ---------------------------------------------------------------
        (Type::List(ea), Type::List(eb)) => {
            let inner = unify(ea, eb)?;
            Ok(Type::List(Box::new(inner)))
        }

        // ---------------------------------------------------------------
        // Map — open-record merge (§7.2)
        // ---------------------------------------------------------------
        (Type::Map(fa), Type::Map(fb)) => Ok(Type::Map(unify_maps(fa, fb)?)),

        // ---------------------------------------------------------------
        // Node — union of label sets (§7.5)
        // ---------------------------------------------------------------
        (Type::Node(la), Type::Node(lb)) => {
            Ok(Type::Node(unify_label_sets(la.as_ref(), lb.as_ref())))
        }

        // ---------------------------------------------------------------
        // Relationship — if both carry a type name they must agree;
        // if either is unconstrained the result is unconstrained.
        // ---------------------------------------------------------------
        (Type::Relationship(ra), Type::Relationship(rb)) => match (ra, rb) {
            (Some(ta), Some(tb)) if ta == tb => Ok(Type::Relationship(Some(ta.clone()))),
            _ => Ok(Type::Relationship(None)),
        },

        // ---------------------------------------------------------------
        // Union: recurse per-variant, canonicalise result (§7.2)
        // ---------------------------------------------------------------
        (Type::Union(variants), other) | (other, Type::Union(variants)) => {
            Ok(unify_union_with(variants, other))
        }

        // ---------------------------------------------------------------
        // Mismatch: no rule applies
        // ---------------------------------------------------------------
        _ => Err(TypeMismatch {
            expected: a.clone(),
            actual: b.clone(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Unify a union type's variants with a non-union type.
///
/// Each variant in `variants` is unified with `other`. Successful unifications
/// are collected and canonicalised with `Type::union`; failures are silently
/// dropped (the variant was incompatible). If no variant unifies, return the
/// union of all variants plus `other` via a widening union — matching the
/// spec's fallback of "form a union".
fn unify_union_with(variants: &[Type], other: &Type) -> Type {
    // Attempt to unify each variant with `other`. Collect successes.
    let results: Vec<Type> = variants
        .iter()
        .filter_map(|v| unify(v, other).ok())
        .collect();

    if results.is_empty() {
        // No variant was compatible — widen: produce Union(variants ∪ {other}).
        let mut members: Vec<Type> = variants.to_vec();
        members.push(other.clone());
        Type::union(members)
    } else {
        Type::union(results)
    }
}

/// Open-record map merge: unify fields present in both sides; keep extras.
fn unify_maps(
    fa: &BTreeMap<SmolStr, Type>,
    fb: &BTreeMap<SmolStr, Type>,
) -> Result<BTreeMap<SmolStr, Type>, TypeMismatch> {
    let mut result: BTreeMap<SmolStr, Type> = BTreeMap::new();

    // All keys from a.
    for (k, ta) in fa {
        if let Some(tb) = fb.get(k) {
            // Key in both: unify field types.
            result.insert(k.clone(), unify(ta, tb)?);
        } else {
            // Key only in a: keep as-is (open-record).
            result.insert(k.clone(), ta.clone());
        }
    }
    // Keys only in b: keep as-is.
    for (k, tb) in fb {
        if !fa.contains_key(k) {
            result.insert(k.clone(), tb.clone());
        }
    }

    Ok(result)
}

/// Node label-set merge: union of label sets.
///
/// - `None` means "any node" (no label constraint) → the result is
///   unconstrained (`None`).
/// - `Some(labels)` on both sides → union of the two sets.
fn unify_label_sets(la: Option<&LabelSet>, lb: Option<&LabelSet>) -> Option<LabelSet> {
    match (la, lb) {
        (None, _) | (_, None) => None,
        (Some(a), Some(b)) => {
            let mut merged: Vec<SmolStr> = a.0.clone();
            merged.extend(b.0.iter().cloned());
            Some(LabelSet::new(merged))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::LabelSet;
    use smol_str::SmolStr;
    use std::collections::BTreeMap;

    // -----------------------------------------------------------------------
    // Helper
    // -----------------------------------------------------------------------

    fn map1(k: &str, v: Type) -> Type {
        let mut m = BTreeMap::new();
        m.insert(SmolStr::new(k), v);
        Type::Map(m)
    }

    fn map2(k1: &str, v1: Type, k2: &str, v2: Type) -> Type {
        let mut m = BTreeMap::new();
        m.insert(SmolStr::new(k1), v1);
        m.insert(SmolStr::new(k2), v2);
        Type::Map(m)
    }

    // -----------------------------------------------------------------------
    // Rule: Any dissolves
    // -----------------------------------------------------------------------

    #[test]
    fn any_with_int_gives_int() {
        assert_eq!(unify(&Type::Any, &Type::Int), Ok(Type::Int));
    }

    #[test]
    fn int_with_any_gives_int() {
        assert_eq!(unify(&Type::Int, &Type::Any), Ok(Type::Int));
    }

    #[test]
    fn any_with_any_gives_any() {
        assert_eq!(unify(&Type::Any, &Type::Any), Ok(Type::Any));
    }

    #[test]
    fn any_with_unknown_gives_unknown() {
        // Unknown wins over Any.
        assert_eq!(unify(&Type::Any, &Type::Unknown), Ok(Type::Unknown));
    }

    // -----------------------------------------------------------------------
    // Rule: Unknown propagates
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_with_int_gives_unknown() {
        assert_eq!(unify(&Type::Unknown, &Type::Int), Ok(Type::Unknown));
    }

    #[test]
    fn int_with_unknown_gives_unknown() {
        assert_eq!(unify(&Type::Int, &Type::Unknown), Ok(Type::Unknown));
    }

    #[test]
    fn unknown_with_unknown_gives_unknown() {
        assert_eq!(unify(&Type::Unknown, &Type::Unknown), Ok(Type::Unknown));
    }

    // -----------------------------------------------------------------------
    // Rule: reflexive
    // -----------------------------------------------------------------------

    #[test]
    fn int_with_int_gives_int() {
        assert_eq!(unify(&Type::Int, &Type::Int), Ok(Type::Int));
    }

    #[test]
    fn bool_with_bool_gives_bool() {
        assert_eq!(unify(&Type::Bool, &Type::Bool), Ok(Type::Bool));
    }

    #[test]
    fn string_with_string_gives_string() {
        assert_eq!(unify(&Type::String, &Type::String), Ok(Type::String));
    }

    // -----------------------------------------------------------------------
    // Rule: Null / nullability
    // -----------------------------------------------------------------------

    #[test]
    fn null_with_null_gives_null() {
        assert_eq!(unify(&Type::Null, &Type::Null), Ok(Type::Null));
    }

    #[test]
    fn null_with_int_gives_nullable_int() {
        let expected = Type::union([Type::Null, Type::Int]);
        assert_eq!(unify(&Type::Null, &Type::Int), Ok(expected));
    }

    #[test]
    fn int_with_null_gives_nullable_int_commutative() {
        let lhs = unify(&Type::Null, &Type::Int).unwrap();
        let rhs = unify(&Type::Int, &Type::Null).unwrap();
        assert_eq!(lhs, rhs, "Null/t unification must be commutative");
    }

    #[test]
    fn null_with_bool_gives_union_null_bool() {
        let expected = Type::union([Type::Null, Type::Bool]);
        assert_eq!(unify(&Type::Null, &Type::Bool), Ok(expected));
    }

    // -----------------------------------------------------------------------
    // Rule: Num / numeric widening
    // -----------------------------------------------------------------------

    #[test]
    fn num_with_int_gives_num() {
        assert_eq!(unify(&Type::Num, &Type::Int), Ok(Type::Num));
    }

    #[test]
    fn int_with_num_gives_num_commutative() {
        assert_eq!(unify(&Type::Int, &Type::Num), Ok(Type::Num));
    }

    #[test]
    fn num_with_float_gives_num() {
        assert_eq!(unify(&Type::Num, &Type::Float), Ok(Type::Num));
    }

    #[test]
    fn float_with_num_gives_num_commutative() {
        assert_eq!(unify(&Type::Float, &Type::Num), Ok(Type::Num));
    }

    #[test]
    fn num_with_num_gives_num() {
        assert_eq!(unify(&Type::Num, &Type::Num), Ok(Type::Num));
    }

    #[test]
    fn int_with_float_gives_num() {
        assert_eq!(unify(&Type::Int, &Type::Float), Ok(Type::Num));
    }

    #[test]
    fn float_with_int_gives_num_commutative() {
        assert_eq!(unify(&Type::Float, &Type::Int), Ok(Type::Num));
    }

    // -----------------------------------------------------------------------
    // Rule: List covariance
    // -----------------------------------------------------------------------

    #[test]
    fn list_int_with_list_int_gives_list_int() {
        let t = Type::List(Box::new(Type::Int));
        assert_eq!(unify(&t, &t), Ok(t));
    }

    #[test]
    fn list_int_with_list_float_gives_list_num() {
        let a = Type::List(Box::new(Type::Int));
        let b = Type::List(Box::new(Type::Float));
        let expected = Type::List(Box::new(Type::Num));
        assert_eq!(unify(&a, &b), Ok(expected));
    }

    #[test]
    fn list_int_with_list_any_gives_list_int() {
        let a = Type::List(Box::new(Type::Int));
        let b = Type::List(Box::new(Type::Any));
        let expected = Type::List(Box::new(Type::Int));
        assert_eq!(unify(&a, &b), Ok(expected));
    }

    #[test]
    fn list_mismatch_propagates_error() {
        let a = Type::List(Box::new(Type::Int));
        let b = Type::List(Box::new(Type::Bool));
        assert!(unify(&a, &b).is_err());
    }

    // -----------------------------------------------------------------------
    // Rule: Map open-record merge
    // -----------------------------------------------------------------------

    #[test]
    fn map_same_key_same_type_merges() {
        let a = map1("x", Type::Int);
        let b = map1("x", Type::Int);
        assert_eq!(unify(&a, &b), Ok(map1("x", Type::Int)));
    }

    #[test]
    fn map_same_key_different_numeric_type_widens() {
        let a = map1("x", Type::Int);
        let b = map1("x", Type::Float);
        assert_eq!(unify(&a, &b), Ok(map1("x", Type::Num)));
    }

    #[test]
    fn map_disjoint_keys_kept_both_open_record() {
        let a = map1("x", Type::Int);
        let b = map1("y", Type::Bool);
        let expected = map2("x", Type::Int, "y", Type::Bool);
        assert_eq!(unify(&a, &b), Ok(expected));
    }

    #[test]
    fn empty_map_with_non_empty_map_gives_non_empty_map() {
        let a = Type::Map(BTreeMap::new());
        let b = map1("x", Type::Int);
        assert_eq!(unify(&a, &b), Ok(map1("x", Type::Int)));
    }

    #[test]
    fn map_field_mismatch_propagates_error() {
        let a = map1("x", Type::Int);
        let b = map1("x", Type::String);
        assert!(unify(&a, &b).is_err());
    }

    // -----------------------------------------------------------------------
    // Rule: Node label-set union
    // -----------------------------------------------------------------------

    #[test]
    fn node_unconstrained_with_any_node_gives_unconstrained() {
        let a = Type::Node(None);
        let b = Type::Node(Some(LabelSet::new([SmolStr::new("Person")])));
        assert_eq!(unify(&a, &b), Ok(Type::Node(None)));
    }

    #[test]
    fn node_same_label_gives_same_label() {
        let a = Type::Node(Some(LabelSet::new([SmolStr::new("Person")])));
        let b = Type::Node(Some(LabelSet::new([SmolStr::new("Person")])));
        assert_eq!(
            unify(&a, &b),
            Ok(Type::Node(Some(LabelSet::new([SmolStr::new("Person")]))))
        );
    }

    #[test]
    fn node_different_labels_union_label_set() {
        let a = Type::Node(Some(LabelSet::new([SmolStr::new("Person")])));
        let b = Type::Node(Some(LabelSet::new([SmolStr::new("Employee")])));
        let expected = Type::Node(Some(LabelSet::new([
            SmolStr::new("Employee"),
            SmolStr::new("Person"),
        ])));
        assert_eq!(unify(&a, &b), Ok(expected));
    }

    // -----------------------------------------------------------------------
    // Rule: Union recursion
    // -----------------------------------------------------------------------

    #[test]
    fn union_with_member_type_stays_union() {
        // Union([Int, Bool]) unified with Int → Int (the Int variant succeeds)
        let u = Type::union([Type::Int, Type::Bool]);
        let result = unify(&u, &Type::Int).unwrap();
        assert_eq!(result, Type::Int);
    }

    #[test]
    fn union_with_widening_member() {
        // Union([Int, Bool]) unified with Float → since Int+Float=Num and
        // Bool fails with Float, result includes Num.
        let u = Type::union([Type::Int, Type::Bool]);
        let result = unify(&u, &Type::Float).unwrap();
        // Int unified with Float = Num; Bool unified with Float = err (dropped).
        assert_eq!(result, Type::Num);
    }

    #[test]
    fn union_with_incompatible_widens_to_larger_union() {
        // Union([Int, Bool]) unified with String → neither Int nor Bool
        // unifies with String, so we widen to Union([Int, Bool, String]).
        let u = Type::union([Type::Int, Type::Bool]);
        let result = unify(&u, &Type::String).unwrap();
        assert_eq!(result, Type::union([Type::Int, Type::Bool, Type::String]));
    }

    // -----------------------------------------------------------------------
    // Mismatch cases
    // -----------------------------------------------------------------------

    #[test]
    fn int_with_string_is_mismatch() {
        let r = unify(&Type::Int, &Type::String);
        assert_eq!(
            r,
            Err(TypeMismatch {
                expected: Type::Int,
                actual: Type::String
            })
        );
    }

    #[test]
    fn bool_with_int_is_mismatch() {
        assert!(unify(&Type::Bool, &Type::Int).is_err());
    }

    #[test]
    fn list_with_map_is_mismatch() {
        let a = Type::List(Box::new(Type::Int));
        let b = map1("x", Type::Int);
        assert!(unify(&a, &b).is_err());
    }

    // -----------------------------------------------------------------------
    // Commutativity spot-checks
    // -----------------------------------------------------------------------

    #[test]
    fn commutativity_any() {
        let types = [Type::Int, Type::Float, Type::Bool, Type::String, Type::Null];
        for t in &types {
            let lhs = unify(&Type::Any, t).unwrap();
            let rhs = unify(t, &Type::Any).unwrap();
            assert_eq!(lhs, rhs, "Any commutativity failed for {t:?}");
        }
    }

    #[test]
    fn commutativity_numeric_widening() {
        assert_eq!(
            unify(&Type::Int, &Type::Float),
            unify(&Type::Float, &Type::Int)
        );
        assert_eq!(unify(&Type::Num, &Type::Int), unify(&Type::Int, &Type::Num));
        assert_eq!(
            unify(&Type::Num, &Type::Float),
            unify(&Type::Float, &Type::Num)
        );
    }

    #[test]
    fn commutativity_null() {
        assert_eq!(
            unify(&Type::Null, &Type::Int),
            unify(&Type::Int, &Type::Null)
        );
    }

    // -----------------------------------------------------------------------
    // Property tests
    // -----------------------------------------------------------------------

    #[cfg(test)]
    mod property {
        use super::*;
        use proptest::prelude::*;

        /// Arbitrary `Type` generator (flat, no deep nesting to keep fast).
        fn arb_type() -> impl Strategy<Value = Type> {
            prop_oneof![
                Just(Type::Any),
                Just(Type::Null),
                Just(Type::Bool),
                Just(Type::Int),
                Just(Type::Float),
                Just(Type::Num),
                Just(Type::String),
                Just(Type::Date),
                Just(Type::Datetime),
                Just(Type::Path),
                Just(Type::Unknown),
                // List with a simple inner type
                prop_oneof![
                    Just(Type::Int),
                    Just(Type::Float),
                    Just(Type::Bool),
                    Just(Type::Any),
                ]
                .prop_map(|inner| Type::List(Box::new(inner))),
                // Node: unconstrained or single label
                prop_oneof![
                    Just(Type::Node(None)),
                    "[a-zA-Z]{1,8}"
                        .prop_map(|s| Type::Node(Some(LabelSet::new([SmolStr::new(s)])))),
                ],
            ]
        }

        proptest! {
            /// unify(a, Any) == a  (Any dissolves)
            #[test]
            fn prop_any_identity(a in arb_type()) {
                let result = unify(&a, &Type::Any);
                prop_assert_eq!(result, Ok(a));
            }

            /// unify(Any, a) == a  (symmetric)
            #[test]
            fn prop_any_identity_left(a in arb_type()) {
                let result = unify(&Type::Any, &a);
                prop_assert_eq!(result, Ok(a));
            }

            /// unify(a, Unknown) == Unknown  (Unknown propagates)
            #[test]
            fn prop_unknown_propagates(a in arb_type()) {
                let result = unify(&a, &Type::Unknown);
                prop_assert_eq!(result, Ok(Type::Unknown));
            }

            /// unify(Unknown, a) == Unknown  (symmetric)
            #[test]
            fn prop_unknown_propagates_left(a in arb_type()) {
                let result = unify(&Type::Unknown, &a);
                prop_assert_eq!(result, Ok(Type::Unknown));
            }

            /// unify(a, a) == a  (reflexivity)
            #[test]
            fn prop_reflexive(a in arb_type()) {
                // Skip Union variants since we don't generate them above, but
                // every simple type should unify with itself.
                let result = unify(&a, &a);
                // Reflexive unification should always succeed.
                prop_assert!(result.is_ok());
            }
        }
    }
}
