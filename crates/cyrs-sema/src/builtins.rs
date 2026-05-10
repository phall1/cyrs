//! Built-in function signatures for the schema-free sema pass (spec §7.4).
//!
//! The schema-free inference pass ([`crate::infer`]) needs to know the
//! signatures of the standard-library functions the language defines
//! without any schema input. `id(n)`, `size(x)`, `head(xs)`, `tail(xs)`,
//! `last(xs)`, `keys(x)`, and `values(map)` are the v1 cap — all other
//! function calls fall through to the schema-aware pass, which queries
//! the caller's [`cyrs_schema::SchemaProvider`].
//!
//! The table is intentionally small and append-only: new entries land at
//! the end. Each [`Builtin`] entry is a plain `struct` literal so the
//! table can live in a `static` array without allocator contact.
//!
//! # Generics
//!
//! Cypher's stdlib is lightly polymorphic: `head`, `tail`, and `last`
//! accept `LIST<T>` and return `T` / `LIST<T>`. Until the sema type
//! system models a real generic `T` (a follow-up bead), we approximate
//! with [`ArgShape::List`] for the parameter slot and
//! [`ReturnShape::ListElement`] / [`ReturnShape::ListSelf`] to signal
//! "element-of-the-input" / "same-as-the-input-list" at the call site.
//! `size` is the only overloaded entry: it accepts either a list or a
//! string and always returns [`Type::Int`].
//!
//! ```no_run
//! use cyrs_sema::builtins::{lookup, Builtin};
//! let b: Option<&'static Builtin> = lookup("size");
//! assert!(b.is_some());
//! ```

use smol_str::SmolStr;

use crate::ty::Type;

/// Parameter shape — what the call site's argument must conform to.
///
/// The variants are intentionally coarse: a full type-schema for
/// built-ins requires generics, which the v1 sema does not yet model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgShape {
    /// Any concrete type (`Node`, `Relationship`, `Path`, `Int`, `String`, …).
    Any,
    /// Must be a `LIST<_>` of any element type.
    List,
    /// Must be a `STRING`.
    String,
    /// Overload of `List | String` — e.g. `size(x)`.
    ListOrString,
    /// Must be a graph entity — `Node`, `Relationship`, or `Path`.
    ///
    /// Used by `id(x)` (cy-zo9.1) to reject scalars. `Any` / `Unknown`
    /// are accepted to avoid cascading diagnostics when inference has
    /// already failed upstream.
    GraphEntity,
    /// Must be a `MAP`.
    ///
    /// Used by `values(map)` (cy-afo) to reject scalars, lists, and
    /// graph entities. `Any` / `Unknown` are accepted to avoid cascades.
    Map,
    /// Must be a graph entity (`Node` / `Relationship`) or a `MAP`.
    ///
    /// Used by `keys(x)` (cy-afo): openCypher defines `keys` over both
    /// graph entities (property-key names) and maps (map keys). `Path`
    /// is deliberately excluded — `keys` is not defined for paths.
    /// `Any` / `Unknown` are accepted to avoid cascades.
    GraphEntityOrMap,
}

impl ArgShape {
    /// Check whether `ty` conforms to this parameter shape.
    ///
    /// Returns `true` when the argument is permitted. `Any` and `Unknown`
    /// always satisfy every shape — schema-free inference cannot rule
    /// them out, and emitting a diagnostic here would cascade.
    #[must_use]
    pub fn accepts(self, ty: &Type) -> bool {
        match self {
            ArgShape::Any => true,
            ArgShape::List => matches!(ty, Type::List(_) | Type::Any | Type::Unknown),
            ArgShape::String => matches!(ty, Type::String | Type::Any | Type::Unknown),
            ArgShape::ListOrString => {
                matches!(ty, Type::List(_) | Type::String | Type::Any | Type::Unknown)
            }
            ArgShape::GraphEntity => matches!(
                ty,
                Type::Node(_) | Type::Relationship(_) | Type::Path | Type::Any | Type::Unknown
            ),
            ArgShape::Map => matches!(ty, Type::Map(_) | Type::Any | Type::Unknown),
            ArgShape::GraphEntityOrMap => matches!(
                ty,
                Type::Node(_) | Type::Relationship(_) | Type::Map(_) | Type::Any | Type::Unknown
            ),
        }
    }

    /// Human-readable label for a parameter shape — used in diagnostic
    /// messages (e.g. `expected Node | Relationship | Path`).
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            ArgShape::Any => "Any",
            ArgShape::List => "List",
            ArgShape::String => "String",
            ArgShape::ListOrString => "List | String",
            ArgShape::GraphEntity => "Node | Relationship | Path",
            ArgShape::Map => "Map",
            ArgShape::GraphEntityOrMap => "Node | Relationship | Map",
        }
    }
}

/// Return shape — what the call site's result type is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnShape {
    /// Concrete fixed type — used for `id: Int` and `size: Int`.
    Fixed(FixedTy),
    /// Element type of the first list argument. Used by `head` and `last`.
    ListElement,
    /// Same list type as the first argument. Used by `tail`.
    ListSelf,
}

/// The subset of [`Type`] the builtin table needs to name.
///
/// A reduced mirror of [`Type`] that can sit inside a `const` context.
/// When a richer signature is needed, extend this enum (or lift the
/// table to a `OnceLock<Vec<…>>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedTy {
    /// `INTEGER`.
    Int,
    /// `STRING`.
    String,
    /// `BOOLEAN`.
    Bool,
    /// `ANY` — the universal super-type.
    Any,
    /// `LIST<STRING>` — returned by `keys`.
    ListString,
    /// `LIST<ANY>` — returned by `values`.
    ListAny,
}

impl FixedTy {
    /// Lift into a full [`Type`].
    #[must_use]
    pub fn to_type(self) -> Type {
        match self {
            FixedTy::Int => Type::Int,
            FixedTy::String => Type::String,
            FixedTy::Bool => Type::Bool,
            FixedTy::Any => Type::Any,
            FixedTy::ListString => Type::List(Box::new(Type::String)),
            FixedTy::ListAny => Type::List(Box::new(Type::Any)),
        }
    }
}

/// One built-in signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builtin {
    /// Canonical (lower-case) name used for lookup.
    pub name: &'static str,
    /// Parameter shapes, positionally.
    pub params: &'static [ArgShape],
    /// What the result type is.
    pub ret: ReturnShape,
    /// Doc string — one-liner, surfaces in LSP hovers.
    pub doc: &'static str,
}

impl Builtin {
    /// Number of positional parameters.
    #[must_use]
    pub fn arity(&self) -> usize {
        self.params.len()
    }

    /// Resolve the return type given the (already-inferred) argument
    /// types. Falls back to [`Type::Any`] when the element / self shape
    /// cannot be concretely recovered — the conservative default.
    #[must_use]
    pub fn resolve_return(&self, args: &[Type]) -> Type {
        match self.ret {
            ReturnShape::Fixed(ft) => ft.to_type(),
            ReturnShape::ListElement => args.first().map_or(Type::Any, |t| match t {
                Type::List(inner) => (**inner).clone(),
                _ => Type::Any,
            }),
            ReturnShape::ListSelf => args.first().map_or_else(
                || Type::List(Box::new(Type::Any)),
                |t| match t {
                    Type::List(_) => t.clone(),
                    _ => Type::List(Box::new(Type::Any)),
                },
            ),
        }
    }
}

// ===========================================================================
// The table
// ===========================================================================

/// The full built-in table. Ordering is append-only: new entries land at
/// the end. Lookups use case-insensitive name comparison via [`lookup`].
pub const BUILTINS: &[Builtin] = &[
    // --- cy-zo9 / cy-zo9.1: graph-element identity ------------------------
    //
    // `id(n)` — opaque identifier of a node, relationship, or path.
    // The argument must be a graph entity; scalars are rejected with
    // E5012 at the `infer` call-site (cy-zo9.1).
    Builtin {
        name: "id",
        params: &[ArgShape::GraphEntity],
        ret: ReturnShape::Fixed(FixedTy::Int),
        doc: "opaque identifier of a node, relationship, or path element",
    },
    // --- cy-5gh: list / string stdlib -------------------------------------
    //
    // `size(x)` — length of a list or character count of a string.
    Builtin {
        name: "size",
        params: &[ArgShape::ListOrString],
        ret: ReturnShape::Fixed(FixedTy::Int),
        doc: "length of a list, or character count of a string",
    },
    // `head(xs)` — first element of a list, or `null` when empty.
    Builtin {
        name: "head",
        params: &[ArgShape::List],
        ret: ReturnShape::ListElement,
        doc: "first element of a list, or null when empty",
    },
    // `tail(xs)` — all but the first element (a list).
    Builtin {
        name: "tail",
        params: &[ArgShape::List],
        ret: ReturnShape::ListSelf,
        doc: "all elements of a list except the first (a list)",
    },
    // `last(xs)` — last element of a list, or `null` when empty.
    Builtin {
        name: "last",
        params: &[ArgShape::List],
        ret: ReturnShape::ListElement,
        doc: "last element of a list, or null when empty",
    },
    // --- cy-afo: map stdlib -----------------------------------------------
    //
    // `keys(x)` — openCypher names `keys` over both graph entities
    // (property-key names) and maps (map keys). The schema layer
    // accepts anything (`PropertyType::Any`); this entry is what the
    // schema-free pass uses to reject scalars / lists / paths with
    // E5012 at the call site.
    Builtin {
        name: "keys",
        params: &[ArgShape::GraphEntityOrMap],
        ret: ReturnShape::Fixed(FixedTy::ListString),
        doc: "property-key names of a node, relationship, or map",
    },
    // `values(map)` — openCypher defines this over `MAP` only, returning
    // the value list as `LIST<ANY>`.
    Builtin {
        name: "values",
        params: &[ArgShape::Map],
        ret: ReturnShape::Fixed(FixedTy::ListAny),
        doc: "values of a map as a list",
    },
];

/// Look up a builtin by case-insensitive name.
///
/// Returns the first matching entry, or `None` if no builtin with that
/// name is registered. Names in [`BUILTINS`] are stored lower-case.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name.eq_ignore_ascii_case(name))
}

/// Look up a builtin by interned [`SmolStr`] name — convenience for call
/// sites that already hold one. Forwards to [`lookup`].
#[must_use]
pub fn lookup_smol(name: &SmolStr) -> Option<&'static Builtin> {
    lookup(name.as_str())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_id_is_present() {
        let b = lookup("id").expect("id is a registered builtin");
        assert_eq!(b.name, "id");
        assert_eq!(b.arity(), 1);
        assert_eq!(b.resolve_return(&[Type::Node(None)]), Type::Int);
    }

    /// cy-zo9.1: the `id` builtin's parameter shape must be
    /// `GraphEntity`, which accepts `Node`/`Relationship`/`Path` and
    /// rejects scalar types.
    #[test]
    fn id_param_shape_is_graph_entity() {
        let b = lookup("id").expect("id is registered");
        assert_eq!(b.params, &[ArgShape::GraphEntity]);

        let shape = b.params[0];
        assert!(shape.accepts(&Type::Node(None)));
        assert!(shape.accepts(&Type::Relationship(None)));
        assert!(shape.accepts(&Type::Path));
        assert!(shape.accepts(&Type::Any));
        assert!(shape.accepts(&Type::Unknown));

        assert!(!shape.accepts(&Type::Int));
        assert!(!shape.accepts(&Type::String));
        assert!(!shape.accepts(&Type::Bool));
        assert!(!shape.accepts(&Type::Float));
        assert!(!shape.accepts(&Type::List(Box::new(Type::Int))));
    }

    #[test]
    fn list_shape_accepts_list_only() {
        let shape = ArgShape::List;
        assert!(shape.accepts(&Type::List(Box::new(Type::Int))));
        assert!(shape.accepts(&Type::Any));
        assert!(shape.accepts(&Type::Unknown));
        assert!(!shape.accepts(&Type::Int));
        assert!(!shape.accepts(&Type::String));
    }

    #[test]
    fn list_or_string_accepts_either() {
        let shape = ArgShape::ListOrString;
        assert!(shape.accepts(&Type::List(Box::new(Type::Int))));
        assert!(shape.accepts(&Type::String));
        assert!(!shape.accepts(&Type::Int));
        assert!(!shape.accepts(&Type::Node(None)));
    }

    #[test]
    fn any_shape_accepts_everything() {
        let shape = ArgShape::Any;
        assert!(shape.accepts(&Type::Int));
        assert!(shape.accepts(&Type::String));
        assert!(shape.accepts(&Type::Node(None)));
        assert!(shape.accepts(&Type::List(Box::new(Type::Bool))));
    }

    #[test]
    fn lookup_size_returns_int_for_list() {
        let b = lookup("size").expect("size is a registered builtin");
        assert_eq!(b.arity(), 1);
        assert_eq!(
            b.resolve_return(&[Type::List(Box::new(Type::Int))]),
            Type::Int
        );
    }

    #[test]
    fn lookup_size_returns_int_for_string() {
        let b = lookup("size").expect("size is a registered builtin");
        assert_eq!(b.resolve_return(&[Type::String]), Type::Int);
    }

    #[test]
    fn lookup_head_returns_element_type() {
        let b = lookup("head").expect("head is a registered builtin");
        assert_eq!(b.arity(), 1);
        assert_eq!(
            b.resolve_return(&[Type::List(Box::new(Type::Int))]),
            Type::Int
        );
    }

    #[test]
    fn lookup_head_falls_back_to_any_on_non_list_input() {
        let b = lookup("head").unwrap();
        assert_eq!(b.resolve_return(&[Type::String]), Type::Any);
    }

    #[test]
    fn lookup_tail_returns_list_self() {
        let b = lookup("tail").expect("tail is a registered builtin");
        let lst = Type::List(Box::new(Type::Bool));
        assert_eq!(b.resolve_return(std::slice::from_ref(&lst)), lst);
    }

    #[test]
    fn lookup_tail_defaults_to_list_any_on_non_list() {
        let b = lookup("tail").unwrap();
        assert_eq!(
            b.resolve_return(&[Type::Bool]),
            Type::List(Box::new(Type::Any))
        );
    }

    #[test]
    fn lookup_last_returns_element_type() {
        let b = lookup("last").expect("last is a registered builtin");
        assert_eq!(
            b.resolve_return(&[Type::List(Box::new(Type::String))]),
            Type::String
        );
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(lookup("SIZE").is_some());
        assert!(lookup("Head").is_some());
        assert!(lookup("TAIL").is_some());
        assert!(lookup("Last").is_some());
    }

    #[test]
    fn lookup_unknown_is_none() {
        assert!(lookup("not_a_builtin").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn smol_lookup_delegates() {
        assert!(lookup_smol(&SmolStr::new("size")).is_some());
        assert!(lookup_smol(&SmolStr::new("bogus")).is_none());
    }

    /// The table names are lower-case; preserve that invariant so
    /// lookup-casing behavior is uniform.
    #[test]
    fn all_names_are_lowercase() {
        for b in BUILTINS {
            assert_eq!(b.name, b.name.to_ascii_lowercase(), "name: {}", b.name);
        }
    }

    // --- cy-afo: keys / values over maps ----------------------------------

    #[test]
    fn keys_is_registered_and_accepts_graph_entity_or_map() {
        let b = lookup("keys").expect("keys is a registered builtin");
        assert_eq!(b.arity(), 1);
        assert_eq!(b.params, &[ArgShape::GraphEntityOrMap]);

        let shape = b.params[0];
        assert!(shape.accepts(&Type::Node(None)));
        assert!(shape.accepts(&Type::Relationship(None)));
        assert!(shape.accepts(&Type::Map(std::collections::BTreeMap::new())));
        assert!(shape.accepts(&Type::Any));
        assert!(shape.accepts(&Type::Unknown));

        // `Path` is not in the list — `keys` is not defined for paths
        // (unlike `id`).
        assert!(!shape.accepts(&Type::Path));
        assert!(!shape.accepts(&Type::Int));
        assert!(!shape.accepts(&Type::String));
        assert!(!shape.accepts(&Type::List(Box::new(Type::Int))));
    }

    #[test]
    fn keys_returns_list_of_string() {
        let b = lookup("keys").unwrap();
        assert_eq!(
            b.resolve_return(&[Type::Map(std::collections::BTreeMap::new())]),
            Type::List(Box::new(Type::String))
        );
        assert_eq!(
            b.resolve_return(&[Type::Node(None)]),
            Type::List(Box::new(Type::String))
        );
    }

    #[test]
    fn values_is_registered_and_accepts_map_only() {
        let b = lookup("values").expect("values is a registered builtin");
        assert_eq!(b.arity(), 1);
        assert_eq!(b.params, &[ArgShape::Map]);

        let shape = b.params[0];
        assert!(shape.accepts(&Type::Map(std::collections::BTreeMap::new())));
        assert!(shape.accepts(&Type::Any));
        assert!(shape.accepts(&Type::Unknown));

        // Everything else (including graph entities) is rejected.
        assert!(!shape.accepts(&Type::Node(None)));
        assert!(!shape.accepts(&Type::Relationship(None)));
        assert!(!shape.accepts(&Type::Path));
        assert!(!shape.accepts(&Type::Int));
        assert!(!shape.accepts(&Type::String));
        assert!(!shape.accepts(&Type::List(Box::new(Type::Int))));
    }

    #[test]
    fn values_returns_list_of_any() {
        let b = lookup("values").unwrap();
        assert_eq!(
            b.resolve_return(&[Type::Map(std::collections::BTreeMap::new())]),
            Type::List(Box::new(Type::Any))
        );
    }

    #[test]
    fn fixed_ty_list_variants_lift_correctly() {
        assert_eq!(
            FixedTy::ListString.to_type(),
            Type::List(Box::new(Type::String))
        );
        assert_eq!(FixedTy::ListAny.to_type(), Type::List(Box::new(Type::Any)));
    }
}
