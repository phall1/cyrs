//! Built-in openCypher function catalog (spec §8.3).
//!
//! [`StandardLibrary`] supplies the openCypher-standardised function set
//! (`id`, `type`, `labels`, `keys`, `properties`, the `length`/`size`
//! family, `coalesce`, string/collection/math/aggregation functions) and
//! is independent of any consumer schema. Consumers wrap their own
//! [`SchemaProvider`] with [`StandardLibrary::wrap`] to get the union.
//!
//! # Composition
//!
//! - `StandardLibrary::new()` — stdlib only; useful for schema-free mode
//!   (spec §8.4 — stdlib is still consulted when no schema is supplied).
//! - `StandardLibrary::wrap(inner)` — stdlib ∪ `inner`. Function lookup
//!   tries stdlib first, then falls back to `inner`; every other method
//!   delegates to `inner`. This ordering means stdlib names shadow
//!   consumer-declared overrides by design (spec §8.3 names "the
//!   openCypher-standardized function set"; consumers MUST NOT redefine
//!   them).
//! - All other [`SchemaProvider`] surface (labels, procedures, endpoints,
//!   properties, digest) comes from the wrapped inner schema; stdlib is
//!   function-only.

use std::collections::BTreeMap;
use std::sync::{LazyLock, OnceLock};

use smol_str::SmolStr;

use crate::{
    DynamicReturnFn, EmptySchema, EndpointDecl, FnCategories, FunctionSignature, ParamDecl,
    ProcedureSignature, PropertyDecl, PropertyType, ReturnTy, SchemaProvider,
};

// ============================================================
// Catalog entry
// ============================================================

/// A compact description of a built-in function. Converted to a full
/// [`FunctionSignature`] on demand by [`StandardLibrary::function`]. The
/// catalog is held in a [`LazyLock`] (rather than a `static`) because
/// `PropertyType::List` requires a `Box::new` which is not `const`.
struct BuiltIn {
    name: &'static str,
    params: Vec<(&'static str, PropertyType)>,
    variadic: Option<PropertyType>,
    return_ty: BuiltInReturn,
    categories: FnCategories,
}

enum BuiltInReturn {
    Constant(PropertyType),
    /// Produces a fresh closure per lookup (closures are not `Clone`).
    Dynamic(fn() -> DynamicReturnFn),
}

impl BuiltIn {
    fn to_signature(&self) -> FunctionSignature {
        let params: Vec<ParamDecl> = self
            .params
            .iter()
            .map(|(n, t)| ParamDecl {
                name: SmolStr::new(n),
                ty: t.clone(),
                default: None,
            })
            .collect();
        let variadic = self.variadic.as_ref().map(|t| ParamDecl {
            name: SmolStr::new("args"),
            ty: t.clone(),
            default: None,
        });
        let return_ty = match &self.return_ty {
            BuiltInReturn::Constant(t) => ReturnTy::Constant(t.clone()),
            BuiltInReturn::Dynamic(f) => ReturnTy::Dynamic(f()),
        };
        FunctionSignature {
            name: SmolStr::new(self.name),
            params,
            variadic,
            return_ty,
            categories: self.categories,
        }
    }
}

// ============================================================
// Catalog (openCypher-standard functions)
// ============================================================

const fn pure() -> FnCategories {
    FnCategories {
        pure: true,
        aggregate: false,
        deterministic: true,
    }
}

const fn agg() -> FnCategories {
    FnCategories {
        pure: true,
        aggregate: true,
        deterministic: true,
    }
}

const fn nondet() -> FnCategories {
    FnCategories {
        pure: true,
        aggregate: false,
        deterministic: false,
    }
}

/// The catalog. Names are stored lower-case; openCypher function names
/// are case-insensitive per the TCK. Lookup via [`find_builtin`]
/// normalises the query name. Built once on first access.
///
/// Grouped by the five families spec §8.3 names explicitly.
static CATALOG: LazyLock<Vec<BuiltIn>> = LazyLock::new(|| {
    vec![
        // ---- element accessors ------------------------------------------
        BuiltIn {
            name: "id",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Int),
            categories: pure(),
        },
        BuiltIn {
            name: "type",
            params: vec![("r", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "labels",
            params: vec![("n", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::String))),
            categories: pure(),
        },
        BuiltIn {
            // openCypher: `keys(x)` — accepts a `NODE`, `RELATIONSHIP`,
            // or `MAP`, returning the property-name / map-key list as
            // `LIST<STRING>`. The parameter slot is typed `Any` here
            // because the schema layer has no `Map` variant; the
            // Node / Relationship / Map kind check happens in
            // `cyrs-sema` via `ArgShape::GraphEntityOrMap`.
            name: "keys",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::String))),
            categories: pure(),
        },
        BuiltIn {
            // openCypher: `values(map)` — returns the map's values as
            // `LIST<ANY>`. The kind check (argument must be a `MAP`)
            // lives in `cyrs-sema` via `ArgShape::Map`.
            name: "values",
            params: vec![("map", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::Any))),
            categories: pure(),
        },
        BuiltIn {
            name: "properties",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Any),
            categories: pure(),
        },
        // ---- length / size family ---------------------------------------
        BuiltIn {
            name: "length",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Int),
            categories: pure(),
        },
        BuiltIn {
            name: "size",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Int),
            categories: pure(),
        },
        // ---- coalesce (variadic; narrowest-supertype — see §21.2) -------
        BuiltIn {
            name: "coalesce",
            params: vec![],
            variadic: Some(PropertyType::Any),
            return_ty: BuiltInReturn::Dynamic(|| {
                // First non-`Any` argument type wins; `Any` otherwise. The
                // full "narrowest supertype" rule lives in cyrs-sema's
                // unification engine; this is a pragmatic approximation for
                // the schema layer.
                Box::new(|tys: &[PropertyType]| {
                    tys.iter()
                        .find(|t| !matches!(t, PropertyType::Any))
                        .cloned()
                        .unwrap_or(PropertyType::Any)
                })
            }),
            categories: pure(),
        },
        // ---- string functions -------------------------------------------
        BuiltIn {
            name: "toupper",
            params: vec![("s", PropertyType::String)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "tolower",
            params: vec![("s", PropertyType::String)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "trim",
            params: vec![("s", PropertyType::String)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "ltrim",
            params: vec![("s", PropertyType::String)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "rtrim",
            params: vec![("s", PropertyType::String)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "reverse",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                // `reverse` works on both strings and lists. Return the
                // argument type when it is a string or list; `Any` otherwise.
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(t @ (PropertyType::String | PropertyType::List(_))) => t.clone(),
                    _ => PropertyType::Any,
                })
            }),
            categories: pure(),
        },
        BuiltIn {
            name: "substring",
            params: vec![
                ("original", PropertyType::String),
                ("start", PropertyType::Int),
            ],
            variadic: Some(PropertyType::Int),
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "replace",
            params: vec![
                ("original", PropertyType::String),
                ("search", PropertyType::String),
                ("replace", PropertyType::String),
            ],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "split",
            params: vec![
                ("original", PropertyType::String),
                ("delim", PropertyType::String),
            ],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::String))),
            categories: pure(),
        },
        BuiltIn {
            name: "left",
            params: vec![
                ("original", PropertyType::String),
                ("length", PropertyType::Int),
            ],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "right",
            params: vec![
                ("original", PropertyType::String),
                ("length", PropertyType::Int),
            ],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "tostring",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::String),
            categories: pure(),
        },
        BuiltIn {
            name: "tointeger",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Int),
            categories: pure(),
        },
        BuiltIn {
            name: "tofloat",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "toboolean",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Bool),
            categories: pure(),
        },
        // ---- collection functions ---------------------------------------
        BuiltIn {
            name: "head",
            params: vec![("list", PropertyType::List(Box::new(PropertyType::Any)))],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(PropertyType::List(inner)) => (**inner).clone(),
                    _ => PropertyType::Any,
                })
            }),
            categories: pure(),
        },
        BuiltIn {
            name: "last",
            params: vec![("list", PropertyType::List(Box::new(PropertyType::Any)))],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(PropertyType::List(inner)) => (**inner).clone(),
                    _ => PropertyType::Any,
                })
            }),
            categories: pure(),
        },
        BuiltIn {
            name: "tail",
            params: vec![("list", PropertyType::List(Box::new(PropertyType::Any)))],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(t @ PropertyType::List(_)) => t.clone(),
                    _ => PropertyType::Any,
                })
            }),
            categories: pure(),
        },
        BuiltIn {
            name: "range",
            params: vec![("start", PropertyType::Int), ("end", PropertyType::Int)],
            variadic: Some(PropertyType::Int),
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::Int))),
            categories: pure(),
        },
        BuiltIn {
            name: "nodes",
            params: vec![("path", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::Any))),
            categories: pure(),
        },
        BuiltIn {
            name: "relationships",
            params: vec![("path", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::List(Box::new(PropertyType::Any))),
            categories: pure(),
        },
        // ---- math functions ---------------------------------------------
        BuiltIn {
            name: "abs",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(PropertyType::Int) => PropertyType::Int,
                    Some(PropertyType::Float) => PropertyType::Float,
                    _ => PropertyType::Any,
                })
            }),
            categories: pure(),
        },
        BuiltIn {
            name: "sign",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Int),
            categories: pure(),
        },
        BuiltIn {
            name: "ceil",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "floor",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "round",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "sqrt",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "exp",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "log",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "log10",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "sin",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "cos",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "tan",
            params: vec![("x", PropertyType::Float)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "pi",
            params: vec![],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "e",
            params: vec![],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: pure(),
        },
        BuiltIn {
            name: "rand",
            params: vec![],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: nondet(),
        },
        // ---- aggregation functions --------------------------------------
        BuiltIn {
            name: "count",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Int),
            categories: agg(),
        },
        BuiltIn {
            name: "sum",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(PropertyType::Int) => PropertyType::Int,
                    Some(PropertyType::Float) => PropertyType::Float,
                    _ => PropertyType::Any,
                })
            }),
            categories: agg(),
        },
        BuiltIn {
            name: "avg",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: agg(),
        },
        BuiltIn {
            name: "min",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| tys.first().cloned().unwrap_or(PropertyType::Any))
            }),
            categories: agg(),
        },
        BuiltIn {
            name: "max",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| tys.first().cloned().unwrap_or(PropertyType::Any))
            }),
            categories: agg(),
        },
        BuiltIn {
            name: "collect",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| match tys.first() {
                    Some(t) => PropertyType::List(Box::new(t.clone())),
                    None => PropertyType::List(Box::new(PropertyType::Any)),
                })
            }),
            categories: agg(),
        },
        BuiltIn {
            name: "stdev",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: agg(),
        },
        BuiltIn {
            name: "stdevp",
            params: vec![("x", PropertyType::Any)],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: agg(),
        },
        BuiltIn {
            name: "percentilecont",
            params: vec![
                ("x", PropertyType::Any),
                ("percentile", PropertyType::Float),
            ],
            variadic: None,
            return_ty: BuiltInReturn::Constant(PropertyType::Float),
            categories: agg(),
        },
        BuiltIn {
            name: "percentiledisc",
            params: vec![
                ("x", PropertyType::Any),
                ("percentile", PropertyType::Float),
            ],
            variadic: None,
            return_ty: BuiltInReturn::Dynamic(|| {
                Box::new(|tys: &[PropertyType]| tys.first().cloned().unwrap_or(PropertyType::Any))
            }),
            categories: agg(),
        },
    ]
});

/// Case-insensitive lookup. openCypher function names are case-
/// insensitive; we store the catalog lower-case and normalise once at
/// lookup time using ASCII case folding (all built-in names are ASCII).
fn find_builtin(name: &str) -> Option<&'static BuiltIn> {
    static INDEX: OnceLock<BTreeMap<&'static str, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        CATALOG
            .iter()
            .enumerate()
            .map(|(i, b)| (b.name, i))
            .collect()
    });
    // Fast path: already lower-case ASCII.
    if name.bytes().all(|b| !b.is_ascii_uppercase()) {
        return index.get(name).map(|&i| &CATALOG[i]);
    }
    let lower = name.to_ascii_lowercase();
    index.get(lower.as_str()).map(|&i| &CATALOG[i])
}

// ============================================================
// StandardLibrary
// ============================================================

/// The openCypher built-in function catalog as a [`SchemaProvider`].
///
/// Two constructors:
///
/// - [`StandardLibrary::new`] — stdlib only; labels, endpoints, etc.
///   delegate to an [`EmptySchema`]. Suitable for schema-free mode
///   (spec §8.4).
/// - [`StandardLibrary::wrap`] — stdlib ∪ consumer schema. Function
///   lookup consults the stdlib first; all other surface delegates to
///   the wrapped schema.
///
/// The type is generic over the inner provider so the composition is
/// zero-cost. A `Box<dyn SchemaProvider>` also satisfies
/// `SchemaProvider` (object-safe per spec §8.1), which makes the
/// dynamic form ergonomic.
pub struct StandardLibrary<S: SchemaProvider = EmptySchema> {
    inner: S,
}

impl<S: SchemaProvider + core::fmt::Debug> core::fmt::Debug for StandardLibrary<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StandardLibrary")
            .field("builtin_count", &CATALOG.len())
            .field("inner", &self.inner)
            .finish()
    }
}

impl StandardLibrary<EmptySchema> {
    /// Construct a stdlib-only provider (no labels, no procedures).
    pub fn new() -> Self {
        Self { inner: EmptySchema }
    }
}

impl Default for StandardLibrary<EmptySchema> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: SchemaProvider> StandardLibrary<S> {
    /// Wrap a consumer schema so function lookups see stdlib ∪ inner.
    ///
    /// Stdlib names shadow consumer declarations for the same name.
    /// Consumers MUST NOT redefine openCypher built-ins (spec §8.3).
    pub fn wrap(inner: S) -> Self {
        Self { inner }
    }

    /// Borrow the wrapped inner schema. Useful for callers that need
    /// raw access bypassing the stdlib overlay.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// All built-in function names, sorted. Stable across releases;
    /// exposed for completion and documentation consumers.
    pub fn builtin_names() -> Vec<&'static str> {
        let mut v: Vec<&'static str> = CATALOG.iter().map(|b| b.name).collect();
        v.sort_unstable();
        v
    }

    /// Number of built-ins in the catalog. Test-facing shape check.
    pub fn builtin_count() -> usize {
        CATALOG.len()
    }
}

impl<S: SchemaProvider> SchemaProvider for StandardLibrary<S> {
    fn labels(&self) -> Vec<SmolStr> {
        self.inner.labels()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.inner.relationship_types()
    }

    fn has_label(&self, name: &str) -> bool {
        self.inner.has_label(name)
    }

    fn has_relationship_type(&self, name: &str) -> bool {
        self.inner.has_relationship_type(name)
    }

    fn labels_compatible(&self, labels: &[SmolStr]) -> Option<bool> {
        // Multi-label storage compatibility is a property of the
        // wrapped schema; stdlib adds no labels of its own.
        self.inner.labels_compatible(labels)
    }

    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        self.inner.node_properties(label)
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>> {
        self.inner.relationship_properties(rel_type)
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
        self.inner.relationship_endpoints(rel_type)
    }

    fn inverse_of(&self, rel_type: &str) -> Option<SmolStr> {
        self.inner.inverse_of(rel_type)
    }

    fn label_unique_props(&self, label: &str) -> Vec<Vec<SmolStr>> {
        self.inner.label_unique_props(label)
    }

    fn rel_type_unique_props(&self, rel_type: &str) -> Vec<Vec<SmolStr>> {
        self.inner.rel_type_unique_props(rel_type)
    }

    fn function(&self, name: &str) -> Option<FunctionSignature> {
        if let Some(b) = find_builtin(name) {
            return Some(b.to_signature());
        }
        self.inner.function(name)
    }

    fn procedure(&self, name: &str) -> Option<ProcedureSignature> {
        // StandardLibrary is function-only; procedures come from the
        // wrapped schema (spec §8.3 names only functions).
        self.inner.procedure(name)
    }

    fn schema_digest(&self) -> [u8; 32] {
        // Stdlib contents are fixed at compile time, so the digest is
        // the wrapped schema's digest unchanged. If stdlib ever becomes
        // configurable, mix a stdlib version tag in here.
        self.inner.schema_digest()
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_no_duplicate_names() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for b in CATALOG.iter() {
            assert!(seen.insert(b.name), "duplicate built-in: {}", b.name);
        }
    }

    #[test]
    fn catalog_names_are_lowercase_ascii() {
        for b in CATALOG.iter() {
            assert!(
                b.name
                    .bytes()
                    .all(|c: u8| c.is_ascii() && !c.is_ascii_uppercase()),
                "catalog name not lowercase-ascii: {}",
                b.name
            );
        }
    }

    #[test]
    fn new_resolves_core_functions() {
        let s = StandardLibrary::new();
        for name in [
            "id",
            "type",
            "labels",
            "keys",
            "values",
            "properties",
            "length",
            "size",
            "coalesce",
        ] {
            assert!(s.function(name).is_some(), "missing: {name}");
        }
    }

    /// cy-afo: `values(map)` returns `LIST<ANY>` per openCypher (spec §8.3).
    /// The kind check (argument must be a MAP) lives in cyrs-sema.
    #[test]
    fn values_returns_list_of_any() {
        let s = StandardLibrary::new();
        let sig = s.function("values").expect("values is registered");
        assert_eq!(sig.name, SmolStr::new("values"));
        match sig.return_ty {
            ReturnTy::Constant(PropertyType::List(inner)) => {
                assert_eq!(*inner, PropertyType::Any);
            }
            other => panic!("expected Constant(List(Any)), got {other:?}"),
        }
    }

    #[test]
    fn new_resolves_function_families() {
        let s = StandardLibrary::new();
        // String family.
        for n in ["toUpper", "toLower", "substring", "replace", "split"] {
            assert!(s.function(n).is_some(), "string: {n}");
        }
        // Collection family.
        for n in ["head", "last", "tail", "range", "nodes", "relationships"] {
            assert!(s.function(n).is_some(), "collection: {n}");
        }
        // Math family.
        for n in ["abs", "ceil", "floor", "sqrt", "sin", "pi", "rand"] {
            assert!(s.function(n).is_some(), "math: {n}");
        }
        // Aggregation family.
        for n in ["count", "sum", "avg", "min", "max", "collect"] {
            assert!(s.function(n).is_some(), "agg: {n}");
        }
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let s = StandardLibrary::new();
        assert!(s.function("COUNT").is_some());
        assert!(s.function("Count").is_some());
        assert!(s.function("count").is_some());
        assert!(s.function("cOuNt").is_some());
    }

    #[test]
    fn unknown_function_returns_none() {
        let s = StandardLibrary::new();
        assert!(s.function("fribble").is_none());
    }

    #[test]
    fn aggregation_flag_set_on_aggs() {
        let s = StandardLibrary::new();
        let c = s.function("count").unwrap();
        assert!(c.categories.aggregate);
        let a = s.function("abs").unwrap();
        assert!(!a.categories.aggregate);
    }

    #[test]
    fn rand_is_non_deterministic() {
        let s = StandardLibrary::new();
        let r = s.function("rand").unwrap();
        assert!(!r.categories.deterministic);
    }

    #[test]
    fn dynamic_return_closure_is_fresh_per_lookup() {
        // Each lookup produces a fresh closure (closures are not
        // Clone). Call it twice from two lookups.
        let s = StandardLibrary::new();
        let s1 = s.function("coalesce").unwrap();
        let s2 = s.function("coalesce").unwrap();
        let probe = |sig: FunctionSignature| match sig.return_ty {
            ReturnTy::Dynamic(f) => f(&[PropertyType::String]),
            ReturnTy::Constant(_) => panic!("expected Dynamic"),
        };
        assert_eq!(probe(s1), PropertyType::String);
        assert_eq!(probe(s2), PropertyType::String);
    }

    #[test]
    fn coalesce_dynamic_picks_first_non_any() {
        let s = StandardLibrary::new();
        let sig = s.function("coalesce").unwrap();
        match sig.return_ty {
            ReturnTy::Dynamic(f) => {
                assert_eq!(
                    f(&[PropertyType::Any, PropertyType::Int, PropertyType::String]),
                    PropertyType::Int
                );
                assert_eq!(f(&[]), PropertyType::Any);
                assert_eq!(f(&[PropertyType::Any]), PropertyType::Any);
            }
            ReturnTy::Constant(_) => panic!("coalesce should be Dynamic"),
        }
    }

    #[test]
    fn abs_dynamic_preserves_int_or_float() {
        let s = StandardLibrary::new();
        let sig = s.function("abs").unwrap();
        match sig.return_ty {
            ReturnTy::Dynamic(f) => {
                assert_eq!(f(&[PropertyType::Int]), PropertyType::Int);
                assert_eq!(f(&[PropertyType::Float]), PropertyType::Float);
                assert_eq!(f(&[PropertyType::String]), PropertyType::Any);
            }
            ReturnTy::Constant(_) => panic!("abs should be Dynamic"),
        }
    }

    #[test]
    fn head_dynamic_unwraps_list() {
        let s = StandardLibrary::new();
        let sig = s.function("head").unwrap();
        match sig.return_ty {
            ReturnTy::Dynamic(f) => {
                assert_eq!(
                    f(&[PropertyType::List(Box::new(PropertyType::Int))]),
                    PropertyType::Int
                );
                assert_eq!(f(&[PropertyType::Int]), PropertyType::Any);
            }
            ReturnTy::Constant(_) => panic!("head should be Dynamic"),
        }
    }

    #[test]
    fn collect_dynamic_wraps_in_list() {
        let s = StandardLibrary::new();
        let sig = s.function("collect").unwrap();
        match sig.return_ty {
            ReturnTy::Dynamic(f) => {
                assert_eq!(
                    f(&[PropertyType::Int]),
                    PropertyType::List(Box::new(PropertyType::Int))
                );
            }
            ReturnTy::Constant(_) => panic!("collect should be Dynamic"),
        }
    }

    #[test]
    fn builtin_names_is_sorted_and_complete() {
        let names = StandardLibrary::<EmptySchema>::builtin_names();
        assert_eq!(names.len(), StandardLibrary::<EmptySchema>::builtin_count());
        assert_eq!(names.len(), CATALOG.len());
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    // ---- wrap(inner) composition -------------------------------------

    struct FakeSchema;
    impl SchemaProvider for FakeSchema {
        fn labels(&self) -> Vec<SmolStr> {
            vec![SmolStr::new("Person")]
        }
        fn relationship_types(&self) -> Vec<SmolStr> {
            vec![SmolStr::new("KNOWS")]
        }
        fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
            if label == "Person" {
                Some(vec![PropertyDecl {
                    name: SmolStr::new("name"),
                    ty: PropertyType::String,
                    required: true,
                }])
            } else {
                None
            }
        }
        fn relationship_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
            None
        }
        fn relationship_endpoints(&self, rel: &str) -> Vec<EndpointDecl> {
            if rel == "KNOWS" {
                vec![EndpointDecl {
                    from: SmolStr::new("Person"),
                    to: SmolStr::new("Person"),
                    cardinality: crate::Cardinality::ManyToMany,
                }]
            } else {
                Vec::new()
            }
        }
        fn inverse_of(&self, _: &str) -> Option<SmolStr> {
            None
        }
        fn function(&self, name: &str) -> Option<FunctionSignature> {
            if name == "custom_fn" {
                Some(FunctionSignature {
                    name: SmolStr::new("custom_fn"),
                    params: vec![],
                    variadic: None,
                    return_ty: ReturnTy::Constant(PropertyType::Bool),
                    categories: pure(),
                })
            } else if name == "count" {
                // Try to shadow a built-in; stdlib should win.
                Some(FunctionSignature {
                    name: SmolStr::new("count"),
                    params: vec![],
                    variadic: None,
                    return_ty: ReturnTy::Constant(PropertyType::Bool),
                    categories: pure(),
                })
            } else {
                None
            }
        }
        fn procedure(&self, _: &str) -> Option<ProcedureSignature> {
            None
        }
        fn schema_digest(&self) -> [u8; 32] {
            [1u8; 32]
        }
        fn labels_compatible(&self, labels: &[SmolStr]) -> Option<bool> {
            // `Person` and `Robot` cannot co-exist on one node.
            let has = |n: &str| labels.iter().any(|l| l == n);
            Some(!(has("Person") && has("Robot")))
        }
        fn label_unique_props(&self, label: &str) -> Vec<Vec<SmolStr>> {
            if label == "Person" {
                vec![vec![SmolStr::new("name")]]
            } else {
                Vec::new()
            }
        }
        fn rel_type_unique_props(&self, rel_type: &str) -> Vec<Vec<SmolStr>> {
            if rel_type == "KNOWS" {
                vec![vec![SmolStr::new("since")]]
            } else {
                Vec::new()
            }
        }
    }

    #[test]
    fn wrap_delegates_non_function_surface() {
        let s = StandardLibrary::wrap(FakeSchema);
        assert_eq!(s.labels(), vec![SmolStr::new("Person")]);
        assert!(s.has_label("Person"));
        assert_eq!(s.relationship_types(), vec![SmolStr::new("KNOWS")]);
        assert!(s.has_relationship_type("KNOWS"));
        assert_eq!(
            s.node_properties("Person").unwrap()[0].name,
            SmolStr::new("name")
        );
        assert!(s.relationship_properties("KNOWS").is_none());
        assert_eq!(s.relationship_endpoints("KNOWS").len(), 1);
        assert_eq!(s.schema_digest(), [1u8; 32]);
    }

    #[test]
    fn wrap_delegates_schema_constraint_surface() {
        // `labels_compatible` and `*_unique_props` are pure properties of
        // the wrapped schema; `StandardLibrary` forwards them unchanged
        // (feat-request §2.2 / §2.3).
        let s = StandardLibrary::wrap(FakeSchema);
        assert_eq!(
            s.labels_compatible(&[SmolStr::new("Person"), SmolStr::new("Robot")]),
            Some(false),
        );
        assert_eq!(s.labels_compatible(&[SmolStr::new("Person")]), Some(true));
        assert_eq!(
            s.label_unique_props("Person"),
            vec![vec![SmolStr::new("name")]],
        );
        assert!(s.label_unique_props("Unknown").is_empty());
        assert_eq!(
            s.rel_type_unique_props("KNOWS"),
            vec![vec![SmolStr::new("since")]],
        );
        assert!(s.rel_type_unique_props("UNKNOWN").is_empty());
    }

    #[test]
    fn wrap_stdlib_shadows_inner_function() {
        let s = StandardLibrary::wrap(FakeSchema);
        // Inner returns Bool for `count`; stdlib wins with Int.
        let sig = s.function("count").expect("stdlib has count");
        match sig.return_ty {
            ReturnTy::Constant(PropertyType::Int) => {}
            other => panic!("expected stdlib count -> Int, got {other:?}"),
        }
        assert!(sig.categories.aggregate);
    }

    #[test]
    fn wrap_falls_through_to_inner_for_unknown_stdlib_fn() {
        let s = StandardLibrary::wrap(FakeSchema);
        let sig = s.function("custom_fn").expect("inner fn");
        assert_eq!(sig.name, SmolStr::new("custom_fn"));
        match sig.return_ty {
            ReturnTy::Constant(PropertyType::Bool) => {}
            _ => panic!("expected inner Bool"),
        }
    }

    #[test]
    fn wrap_returns_none_for_truly_unknown() {
        let s = StandardLibrary::wrap(FakeSchema);
        assert!(s.function("not_a_real_function").is_none());
    }

    #[test]
    fn standard_library_is_object_safe() {
        // Force dyn coercion via a function that takes &dyn.
        fn accepts(_: &dyn SchemaProvider) {}
        let s = StandardLibrary::new();
        accepts(&s);
        let w = StandardLibrary::wrap(FakeSchema);
        accepts(&w);
    }
}
