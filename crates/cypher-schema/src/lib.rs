//! `cypher-schema` — the schema surface consumers implement (spec 0001 §8).
//!
//! This crate defines the [`SchemaProvider`] trait and its supporting
//! types. It has no runtime behaviour: it exists so the `cypher-sema`
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
#![doc(html_root_url = "https://docs.rs/cypher-schema/0.0.1")]

use smol_str::SmolStr;

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

    fn has_label(&self, name: &str) -> bool {
        self.labels().iter().any(|l| l == name)
    }

    fn has_relationship_type(&self, name: &str) -> bool {
        self.relationship_types().iter().any(|r| r == name)
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PropertyDecl {
    pub name: SmolStr,
    pub ty: PropertyType,
    pub required: bool,
}

/// The propertable-value type language. Intentionally simpler than the
/// full Cypher value type — schemas describe what values are *stored*.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// structurally. Unifies only with itself and with `Any`.
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
    pub from: SmolStr,
    pub to: SmolStr,
    pub cardinality: Cardinality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
pub struct FunctionSignature {
    pub name: SmolStr,
    pub params: Vec<ParamDecl>,
    pub variadic: Option<ParamDecl>,
    pub return_ty: ReturnTy,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FnCategories {
    pub pure: bool,
    pub aggregate: bool,
    pub deterministic: bool,
}

/// A procedure signature. Procedures have a mode and a `YIELD` column
/// list in addition to inputs.
#[derive(Debug, Clone)]
pub struct ProcedureSignature {
    pub name: SmolStr,
    pub params: Vec<ParamDecl>,
    pub yields: Vec<YieldDecl>,
    pub mode: ProcMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProcMode {
    Read,
    Write,
    Schema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParamDecl {
    pub name: SmolStr,
    pub ty: PropertyType,
    pub default: Option<SmolStr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct YieldDecl {
    pub name: SmolStr,
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
}
