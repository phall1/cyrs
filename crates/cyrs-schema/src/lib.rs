//! `cyrs-schema` — the schema surface consumers implement (spec 0001 §8).
//!
//! This crate defines the [`SchemaProvider`] trait and its supporting
//! types. It has no runtime behaviour: it exists so the `cyrs-sema`
//! pass and the LSP can consult a consumer-owned schema without the
//! consumer pulling Cypher-internal types.
//!
//! Consumers implement [`SchemaProvider`] against their own storage
//! (graph database catalog, TOML spec, JSON document, etc.). The crate
//! intentionally says nothing about where schema data comes from.
//!
//! # Invariants
//!
//! - [`SchemaProvider::schema_digest`] is a content-addressed fingerprint.
//!   Must change on every observable schema change; must be stable across
//!   identical schemas. Used as a Salsa input (spec §11.2).
//! - Label, relationship-type, and property names are Cypher-identifier
//!   strings. Escaping is the caller's responsibility.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cyrs-schema/0.0.1")]

use smol_str::SmolStr;

mod standard_library;
pub use standard_library::StandardLibrary;

mod in_memory;
pub use in_memory::{BuilderError, InMemorySchema, InMemorySchemaBuilder, RelDecl};

#[cfg(feature = "file")]
pub mod file;

pub mod diff;
pub mod lint;

// ============================================================
// SchemaProvider
// ============================================================

/// The single trait consumers implement to feed schema into the front-end.
///
/// The trait is object-safe; the front-end uses `dyn SchemaProvider`
/// internally so a single schema can be shared across Salsa queries. A
/// consumer may cache on their side — the trait assumes method calls are
/// cheap but does not require it.
pub trait SchemaProvider: Send + Sync + 'static {
    /// All declared labels. Order is not semantic; callers sort if they
    /// need deterministic output.
    fn labels(&self) -> Vec<SmolStr>;

    /// All declared relationship types.
    fn relationship_types(&self) -> Vec<SmolStr>;

    /// Convenience predicate: does the schema declare `name` as a label?
    fn has_label(&self, name: &str) -> bool {
        self.labels().iter().any(|l| l == name)
    }

    /// Convenience predicate: does the schema declare `name` as a
    /// relationship type?
    fn has_relationship_type(&self, name: &str) -> bool {
        self.relationship_types().iter().any(|r| r == name)
    }

    /// Whether the schema's storage can hold a node carrying *all* of
    /// `labels` at once (feat-request §2.3).
    ///
    /// `CREATE (n:A:B)` is only valid if labels `A` and `B` can
    /// co-exist on a single node. For a relational embedder this is
    /// whether the labels map to the same — or a join-compatible —
    /// table; for a native graph store every combination is usually
    /// fine.
    ///
    /// Return value:
    ///
    /// - `None` — the provider does not constrain this combination.
    ///   This is the **permissive default**: the front-end accepts the
    ///   label set. A provider that does not model multi-label storage
    ///   should leave this method unimplemented.
    /// - `Some(true)` — the combination is storable; accept.
    /// - `Some(false)` — the combination cannot be stored together;
    ///   the semantic pass rejects it with `E4023`.
    ///
    /// `labels` is the full label set under consideration. A slice of
    /// length 0 or 1 is always trivially storable; providers may still
    /// return `None` for it and the front-end will not reject.
    fn labels_compatible(&self, labels: &[SmolStr]) -> Option<bool> {
        let _ = labels;
        None
    }

    /// Properties declared on a node with this label.
    ///
    /// - `None` — the label is unknown.
    /// - `Some(empty)` — the label is known but no properties are declared
    ///   (schema-less node, or purely structural).
    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>>;

    /// Properties declared on a relationship of this type.
    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>>;

    /// Declared endpoint pairs for a relationship type. Empty = endpoint-
    /// polymorphic; the semantic pass then skips endpoint checks.
    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl>;

    /// Declared inverse relationship type, if any. Consumers that model
    /// typed inverses return them here; others return `None`.
    fn inverse_of(&self, rel_type: &str) -> Option<SmolStr>;

    /// Look up a function signature. Used by typecheck and by completion.
    fn function(&self, name: &str) -> Option<FunctionSignature>;

    /// Look up a procedure signature for `CALL <proc>`.
    fn procedure(&self, name: &str) -> Option<ProcedureSignature>;

    /// A content-addressed digest of the schema's observable surface.
    /// MUST change whenever any declaration visible through this trait
    /// changes.
    fn schema_digest(&self) -> [u8; 32];
}

// ============================================================
// Types
// ============================================================

/// A declared property on a label or relationship type.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new fields (e.g.
/// `documentation`, `default`) can land without forcing a SemVer-major
/// release.  External crates construct via [`PropertyDecl::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct PropertyDecl {
    /// Property name as it appears in `n.prop` expressions.
    pub name: SmolStr,
    /// Declared type (spec §8.2).  Consumers that don't care about
    /// typing can surface `PropertyType::Any`.
    pub ty: PropertyType,
    /// `true` when the schema requires every instance to carry this
    /// property (nullable otherwise).
    pub required: bool,
}

impl PropertyDecl {
    /// Construct a [`PropertyDecl`].
    ///
    /// This is the SemVer-stable constructor; external crates should
    /// prefer it over struct literals so the struct can grow fields
    /// without forcing a SemVer-major release.
    #[must_use]
    pub fn new(name: impl Into<SmolStr>, ty: PropertyType, required: bool) -> Self {
        Self {
            name: name.into(),
            ty,
            required,
        }
    }
}

/// The propertable-value type language. Intentionally simpler than the
/// full Cypher value type — schemas describe what values are *stored*.
///
/// Variants map 1:1 to spec §8.2's type lattice; the variant names
/// are self-documenting.  `#[allow(missing_docs)]` applies to the
/// primitive variants; variants with nontrivial invariants
/// (`Enum`, `Opaque`) keep their own docstrings.
//
// NOTE (cy-2i9.1): heavily matched across `cyrs-sema`.  Marking
// `#[non_exhaustive]` would force wildcard arms at every cross-crate
// match site; deferred to a follow-up bead.  See `docs/stability.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
pub enum PropertyType {
    String,
    Int,
    Float,
    Bool,
    Date,
    Datetime,
    List(Box<PropertyType>),
    /// A closed enum carrying its name and variant names.
    ///
    /// Spec §8.2 shape: `Enum(SmolStr, Vec<SmolStr>)` — tuple variant.
    Enum(SmolStr, Vec<SmolStr>),
    /// An opaque typed value the consumer chooses not to model
    /// structurally.
    ///
    /// **Unification invariant (spec §8.2):** `Opaque(n)` unifies with
    /// `Opaque(n)` (same symbolic name) and with [`PropertyType::Any`];
    /// every other pairing is a type error. This rule lives in the
    /// unification layer (`cyrs-sema`); the shape here only carries the
    /// symbolic name. Two opaque types with different names never unify.
    Opaque(SmolStr),
    /// Fallback: any property value. Equivalent to "type unknown".
    ///
    /// Not in spec §8.2's normative 9-variant set; retained as an
    /// internal fallback for [`ReturnTy::Dynamic`] cloning. Consumers
    /// should prefer the typed variants.
    Any,
}

/// Declared endpoint shape for a relationship type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EndpointDecl {
    /// Source label of a matching pattern (left-hand side of the arrow).
    pub from: SmolStr,
    /// Target label of a matching pattern (right-hand side of the arrow).
    pub to: SmolStr,
    /// Multiplicity between `from` and `to` endpoints.
    pub cardinality: Cardinality,
}

/// Relationship multiplicity between two label endpoints.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new cardinality forms can
/// land without forcing a SemVer-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

/// A function catalog entry.
///
/// The return type is modelled as a closure so consumers can express
/// signature-dependent return inference (e.g., `coalesce(T, T) -> T`).
/// Signature-independent functions return a constant.
///
/// **Spec deviation note (§8.2):** the spec's shorthand is
/// `variadic: Option<Type>`; we use [`ParamDecl`] so the trailing
/// variadic parameter can carry a name and default consistently with
/// `params`. Only `variadic.ty` is semantically significant; `name` is
/// diagnostic-only and `default` is unused.
pub struct FunctionSignature {
    /// Function name as it appears in a `count(…)` call.
    pub name: SmolStr,
    /// Fixed-arity parameter list in declaration order.
    pub params: Vec<ParamDecl>,
    /// Optional trailing variadic parameter (spec §8.2 shorthand).
    pub variadic: Option<ParamDecl>,
    /// How the return type is computed (constant vs argument-derived).
    pub return_ty: ReturnTy,
    /// Purity / determinism flags used by the sema purity checker.
    pub categories: FnCategories,
}

impl core::fmt::Debug for FunctionSignature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FunctionSignature")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("variadic", &self.variadic)
            .field("categories", &self.categories)
            .finish_non_exhaustive()
    }
}

impl Clone for FunctionSignature {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            params: self.params.clone(),
            variadic: self.variadic.clone(),
            return_ty: match &self.return_ty {
                ReturnTy::Constant(t) => ReturnTy::Constant(t.clone()),
                ReturnTy::Dynamic(_) => ReturnTy::Constant(PropertyType::Any),
            },
            categories: self.categories,
        }
    }
}

/// Closure type for dynamic return-type inference.
pub type DynamicReturnFn = Box<dyn Fn(&[PropertyType]) -> PropertyType + Send + Sync>;

/// How a function's return type is computed.
pub enum ReturnTy {
    /// Independent of argument types.
    Constant(PropertyType),
    /// Derived from argument types. The closure receives the caller's
    /// argument types (possibly `Any` where unknown) and returns the
    /// computed return type.
    Dynamic(DynamicReturnFn),
}

impl core::fmt::Debug for ReturnTy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Constant(t) => f.debug_tuple("Constant").field(t).finish(),
            Self::Dynamic(_) => f.debug_tuple("Dynamic").field(&"<fn>").finish(),
        }
    }
}

/// Purity / determinism / aggregation flags for a function.  The
/// sema pass uses these to decide which syntactic positions a
/// function may appear in (aggregates in `RETURN`, pure functions
/// in `WHERE`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FnCategories {
    /// `true` iff the function has no side effects.
    pub pure: bool,
    /// `true` iff the function is an aggregate (e.g. `count`, `sum`).
    pub aggregate: bool,
    /// `true` iff identical inputs always produce identical outputs.
    pub deterministic: bool,
}

/// A procedure signature. Procedures have a mode and a `YIELD` column
/// list in addition to inputs.
#[derive(Debug, Clone)]
pub struct ProcedureSignature {
    /// Procedure name as invoked by `CALL <name>(…)`.
    pub name: SmolStr,
    /// Input parameters in declaration order.
    pub params: Vec<ParamDecl>,
    /// Columns produced by `YIELD`; each row of the call produces a
    /// record with these fields.
    pub yields: Vec<YieldDecl>,
    /// Read / Write / Schema classification (spec §8.2).
    pub mode: ProcMode,
}

/// Procedure access mode (spec §8.2).  Used by sema to gate
/// procedures in read-only contexts.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new modes can land without
/// forcing a SemVer-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum ProcMode {
    Read,
    Write,
    Schema,
}

/// A single parameter of a function or procedure signature.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParamDecl {
    /// Parameter name.  Diagnostic-only for variadic parameters.
    pub name: SmolStr,
    /// Declared parameter type.
    pub ty: PropertyType,
    /// Optional default value as a source-level literal.
    pub default: Option<SmolStr>,
}

/// A single output column of a `YIELD` clause on a procedure call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct YieldDecl {
    /// Column name as it appears after `YIELD`.
    pub name: SmolStr,
    /// Declared column type.
    pub ty: PropertyType,
}

// ============================================================
// Empty schema
// ============================================================

/// A `SchemaProvider` that reports nothing. Useful for schema-free mode
/// and for unit tests that do not want to construct a full schema.
#[derive(Debug, Default)]
pub struct EmptySchema;

impl SchemaProvider for EmptySchema {
    fn labels(&self) -> Vec<SmolStr> {
        Vec::new()
    }
    fn relationship_types(&self) -> Vec<SmolStr> {
        Vec::new()
    }
    fn node_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
        None
    }
    fn relationship_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
        None
    }
    fn relationship_endpoints(&self, _: &str) -> Vec<EndpointDecl> {
        Vec::new()
    }
    fn inverse_of(&self, _: &str) -> Option<SmolStr> {
        None
    }
    fn function(&self, _: &str) -> Option<FunctionSignature> {
        None
    }
    fn procedure(&self, _: &str) -> Option<ProcedureSignature> {
        None
    }
    fn schema_digest(&self) -> [u8; 32] {
        [0u8; 32]
    }
}

// ============================================================
// Static assertions
// ============================================================

/// Compile-time check that [`SchemaProvider`] is object-safe (spec §8.1).
/// Referencing `&dyn SchemaProvider` forces the compiler to verify
/// object-safety; the function itself is never called.
#[doc(hidden)]
pub fn _assert_object_safe(_: &dyn SchemaProvider) {}

/// Compile-time check that [`EmptySchema`] satisfies the trait's
/// `Send + Sync + 'static` bounds.
const _: fn() = || {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<EmptySchema>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_schema_knows_nothing() {
        let s = EmptySchema;
        assert!(s.labels().is_empty());
        assert!(!s.has_label("Person"));
        assert_eq!(s.schema_digest(), [0u8; 32]);
    }

    #[test]
    fn labels_compatible_defaults_to_none() {
        // The default implementation is permissive: `None` means the
        // provider does not constrain the combination (feat-request §2.3).
        let s = EmptySchema;
        assert_eq!(s.labels_compatible(&[]), None);
        assert_eq!(s.labels_compatible(&[SmolStr::new("A")]), None);
        assert_eq!(
            s.labels_compatible(&[SmolStr::new("A"), SmolStr::new("B")]),
            None,
        );
    }

    #[test]
    fn labels_compatible_provider_can_reject_a_combination() {
        // A provider that forbids `A` + `B` co-existing on one node.
        #[derive(Debug)]
        struct ForbidsAB;
        impl SchemaProvider for ForbidsAB {
            fn labels(&self) -> Vec<SmolStr> {
                vec![SmolStr::new("A"), SmolStr::new("B"), SmolStr::new("C")]
            }
            fn relationship_types(&self) -> Vec<SmolStr> {
                Vec::new()
            }
            fn node_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
                None
            }
            fn relationship_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
                None
            }
            fn relationship_endpoints(&self, _: &str) -> Vec<EndpointDecl> {
                Vec::new()
            }
            fn inverse_of(&self, _: &str) -> Option<SmolStr> {
                None
            }
            fn function(&self, _: &str) -> Option<FunctionSignature> {
                None
            }
            fn procedure(&self, _: &str) -> Option<ProcedureSignature> {
                None
            }
            fn schema_digest(&self) -> [u8; 32] {
                [3u8; 32]
            }
            fn labels_compatible(&self, labels: &[SmolStr]) -> Option<bool> {
                let has = |n: &str| labels.iter().any(|l| l == n);
                Some(!(has("A") && has("B")))
            }
        }

        let s = ForbidsAB;
        assert_eq!(
            s.labels_compatible(&[SmolStr::new("A"), SmolStr::new("B")]),
            Some(false),
        );
        assert_eq!(
            s.labels_compatible(&[SmolStr::new("A"), SmolStr::new("C")]),
            Some(true),
        );
    }

    #[test]
    fn property_type_spec_variants_construct() {
        // Spec §8.2: nine normative variants must exist and be
        // constructible. Any additional variants (e.g., `Any`) are
        // implementation-internal fallbacks.
        let specs: [PropertyType; 9] = [
            PropertyType::String,
            PropertyType::Int,
            PropertyType::Float,
            PropertyType::Bool,
            PropertyType::Date,
            PropertyType::Datetime,
            PropertyType::List(Box::new(PropertyType::Int)),
            PropertyType::Enum(SmolStr::new("Color"), vec![SmolStr::new("Red")]),
            PropertyType::Opaque(SmolStr::new("Uuid")),
        ];
        assert_eq!(specs.len(), 9);
        // Round-trip clone + equality on each.
        for t in &specs {
            assert_eq!(t, &t.clone());
        }
    }

    #[test]
    fn property_type_any_fallback_distinct_from_spec_variants() {
        let any = PropertyType::Any;
        assert_ne!(any, PropertyType::String);
        assert_ne!(any, PropertyType::Opaque(SmolStr::new("X")));
    }

    #[test]
    fn cardinality_has_exactly_four_variants() {
        // Exhaustive match: adding a variant without updating this test
        // is a signal that spec §8.2 has grown.
        let all: [Cardinality; 4] = [
            Cardinality::OneToOne,
            Cardinality::OneToMany,
            Cardinality::ManyToOne,
            Cardinality::ManyToMany,
        ];
        for (i, c) in all.iter().enumerate() {
            match c {
                Cardinality::OneToOne
                | Cardinality::OneToMany
                | Cardinality::ManyToOne
                | Cardinality::ManyToMany => {}
            }
            assert_eq!(*c, all[i]);
        }
    }

    #[test]
    fn proc_mode_has_exactly_three_variants() {
        let all: [ProcMode; 3] = [ProcMode::Read, ProcMode::Write, ProcMode::Schema];
        for m in &all {
            match m {
                ProcMode::Read | ProcMode::Write | ProcMode::Schema => {}
            }
        }
        assert_eq!(all[0], ProcMode::Read);
        assert_ne!(ProcMode::Read, ProcMode::Write);
    }

    #[test]
    fn procedure_signature_read_mode_clones_and_compares_on_mode() {
        let sig = ProcedureSignature {
            name: SmolStr::new("db.ping"),
            params: vec![],
            yields: vec![YieldDecl {
                name: SmolStr::new("ok"),
                ty: PropertyType::Bool,
            }],
            mode: ProcMode::Read,
        };
        let cloned = sig.clone();
        assert_eq!(cloned.mode, ProcMode::Read);
        assert_eq!(cloned.yields.len(), 1);
        assert_eq!(cloned.yields[0].ty, PropertyType::Bool);
    }

    #[test]
    fn endpoint_decl_shape() {
        let e = EndpointDecl {
            from: SmolStr::new("Person"),
            to: SmolStr::new("Company"),
            cardinality: Cardinality::ManyToMany,
        };
        assert_eq!(e.clone(), e);
    }

    #[test]
    fn property_decl_shape() {
        let p = PropertyDecl {
            name: SmolStr::new("age"),
            ty: PropertyType::Int,
            required: true,
        };
        assert!(p.required);
        assert_eq!(p.clone(), p);
    }

    #[test]
    fn function_signature_clone_preserves_params_and_categories() {
        // Constant-return path.
        let c = FunctionSignature {
            name: SmolStr::new("size"),
            params: vec![ParamDecl {
                name: SmolStr::new("x"),
                ty: PropertyType::List(Box::new(PropertyType::Any)),
                default: None,
            }],
            variadic: None,
            return_ty: ReturnTy::Constant(PropertyType::Int),
            categories: FnCategories {
                pure: true,
                aggregate: false,
                deterministic: true,
            },
        };
        let cloned = c.clone();
        assert_eq!(cloned.name, c.name);
        assert_eq!(cloned.params, c.params);
        assert_eq!(cloned.categories, c.categories);
        match cloned.return_ty {
            ReturnTy::Constant(PropertyType::Int) => {}
            _ => panic!("expected Constant(Int)"),
        }
    }

    #[test]
    fn function_signature_clone_dynamic_collapses_to_any() {
        // Documented Clone path: ReturnTy::Dynamic cannot clone its
        // closure, so it collapses to Constant(Any). Schema lookups
        // should avoid cloning dynamic signatures on the hot path.
        let d = FunctionSignature {
            name: SmolStr::new("coalesce"),
            params: vec![],
            variadic: Some(ParamDecl {
                name: SmolStr::new("args"),
                ty: PropertyType::Any,
                default: None,
            }),
            return_ty: ReturnTy::Dynamic(Box::new(|tys| {
                tys.first().cloned().unwrap_or(PropertyType::Any)
            })),
            categories: FnCategories {
                pure: true,
                aggregate: false,
                deterministic: true,
            },
        };
        let cloned = d.clone();
        assert_eq!(cloned.params, d.params);
        assert_eq!(cloned.variadic, d.variadic);
        assert_eq!(cloned.categories, d.categories);
        match cloned.return_ty {
            ReturnTy::Constant(PropertyType::Any) => {}
            _ => panic!("Dynamic clone must collapse to Constant(Any)"),
        }
    }
}
