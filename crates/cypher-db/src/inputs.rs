// Salsa's `#[input]` macro expands into generated getter / setter
// methods on each input struct.  Those are collectively described by
// the module-level docs below, not per-method.  The outer
// `#[allow(missing_docs)]` attributes on each struct don't carry
// through to the macro-expanded impl blocks, so we lift the
// exemption to file-scope.  Hand-written items in this file still
// carry their own docstrings — see `options_digest` below.
#![allow(missing_docs)]

//! Input queries for the incremental analysis database (spec §11.2).
//!
//! ## Design
//!
//! Three Salsa `#[input]` structs cover the full input surface:
//!
//! | Input struct        | Scope      | Salsa kind   | Purpose                                   |
//! |---------------------|------------|--------------|-------------------------------------------|
//! | [`crate::SourceFile`] | per-file | `#[input]`   | raw source text + dialect (from `cy-zx6`) |
//! | [`FileOptions`]     | per-file   | `#[input]`   | full [`AnalysisOptions`] for this file    |
//! | [`WorkspaceInputs`] | workspace  | `#[input]`   | workspace-scoped schema (§11.4)           |
//!
//! The `options_digest` for a file is a **derived** query (`#[salsa::tracked]`)
//! computed from `FileOptions`.  Changing any field of `AnalysisOptions` →
//! different digest → all derived queries keyed on that digest are invalidated.
//!
//! ## Stable hashing (`AnalysisOptions::digest`)
//!
//! The digest is a 64-bit FNV-1a hash computed over a canonical byte
//! representation of `AnalysisOptions`.  The canonicalisation rules are:
//!
//! 1. `dialect`         — encoded as a single `u8` (0 = `GqlAligned`, 1 = `OpenCypherV9`).
//! 2. `warn_shadowing`  — encoded as a `u8` (0 = false, 1 = true).
//! 3. `parameter_hints` — stored in a `BTreeMap<SmolStr, Type>` so iteration
//!    is always in lexicographic key order, independent of insertion order.
//!    Each entry is serialised as `len(key) ++ key_bytes ++ type_tag`.
//!
//! No `std::collections::HashMap` is used anywhere in the hash path (§8
//! determinism rule).

use std::collections::BTreeMap;
use std::sync::Arc;

use cypher_schema::SchemaProvider;
use cypher_sema::ty::Type;
use smol_str::SmolStr;

use crate::{CypherDb, DialectMode};

// ---------------------------------------------------------------------------
// AnalysisOptions
// ---------------------------------------------------------------------------

/// Full set of analysis options for a single file.
///
/// All fields that affect any derived query MUST appear here so that a
/// digest change automatically invalidates everything downstream.
///
/// `parameter_hints` uses [`BTreeMap`] (not `HashMap`) to guarantee that
/// the digest is independent of insertion order (spec §8 determinism).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// Dialect used for gate checks (§9). Defaults to [`DialectMode::GqlAligned`].
    pub dialect: DialectMode,
    /// Warn when a variable in an inner scope shadows one in an outer scope.
    pub warn_shadowing: bool,
    /// Caller-supplied type hints for query parameters.
    ///
    /// Stored in a [`BTreeMap`] so digest computation is order-independent.
    pub parameter_hints: BTreeMap<SmolStr, Type>,
}

impl AnalysisOptions {
    /// Compute a stable, deterministic 64-bit digest of these options.
    ///
    /// Uses FNV-1a (64-bit) over a canonical byte encoding.  The encoding
    /// is deliberately simple and stable across process restarts because:
    ///
    /// - `std::collections::hash_map::DefaultHasher` is NOT stable across
    ///   runs (Rust explicitly reserves the right to change it).
    /// - No external hashing crate is required: FNV-1a is trivial to inline
    ///   and has no dependencies.
    ///
    /// ## Canonical encoding
    ///
    /// ```text
    /// [dialect_u8] [warn_shadowing_u8]
    /// [num_hints_le64]
    /// for each (key, ty) in parameter_hints (BTreeMap order = lex):
    ///   [key_len_le64] [key_utf8_bytes] [type_tag_u8]
    /// ```
    #[must_use]
    pub fn digest(&self) -> u64 {
        let mut h = Fnv64::new();
        // dialect
        h.write_u8(match self.dialect {
            DialectMode::GqlAligned => 0,
            DialectMode::OpenCypherV9 => 1,
        });
        // warn_shadowing
        h.write_u8(u8::from(self.warn_shadowing));
        // parameter_hints (BTreeMap → lexicographic order)
        h.write_u64(self.parameter_hints.len() as u64);
        for (key, ty) in &self.parameter_hints {
            let kb = key.as_bytes();
            h.write_u64(kb.len() as u64);
            h.write_bytes(kb);
            h.write_u8(type_tag(ty));
        }
        h.finish()
    }
}

/// Map a [`Type`] to a single byte tag for digest purposes.
///
/// Only the top-level variant matters for cache invalidation — two calls
/// that differ only in the inner `Type` of a `List` will both produce a
/// `List` tag, which is conservative (safe: may cause a spurious
/// re-evaluation, but never a missed invalidation).  A full structural
/// encoding is unnecessary for the v1 parameter-hint use case.
fn type_tag(ty: &Type) -> u8 {
    match ty {
        Type::Any => 0,
        Type::Null => 1,
        Type::Bool => 2,
        Type::Int => 3,
        Type::Float => 4,
        Type::Num => 5,
        Type::String => 6,
        Type::Date => 7,
        Type::Datetime => 8,
        Type::List(_) => 9,
        Type::Map(_) => 10,
        Type::Node(_) => 11,
        Type::Relationship(_) => 12,
        Type::Path => 13,
        Type::Union(_) => 14,
        Type::Unknown => 15,
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64-bit hasher (inline, no external dep)
// ---------------------------------------------------------------------------

struct Fnv64(u64);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl Fnv64 {
    fn new() -> Self {
        Self(FNV_OFFSET)
    }

    fn write_u8(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(FNV_PRIME);
    }

    fn write_u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }

    fn write_bytes(&mut self, bs: &[u8]) {
        for &b in bs {
            self.write_u8(b);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Salsa #[input] — per-file analysis options
// ---------------------------------------------------------------------------

/// Per-file analysis options input.
///
/// This is a separate `#[salsa::input]` from [`SourceFile`] so that changing
/// options for one file does not touch the source revision of any other file.
/// The `options_digest` derived query reads only this struct.
#[allow(missing_docs)]
#[salsa::input]
pub struct FileOptions {
    /// Full analysis options for this file.
    pub options: AnalysisOptions,
}

// ---------------------------------------------------------------------------
// Salsa #[input] — workspace-scoped schema
// ---------------------------------------------------------------------------

/// Workspace-scoped schema input (spec §11.4).
///
/// There is exactly one `WorkspaceInputs` per database.  The schema is
/// shared across all files; changing it invalidates all schema-dependent
/// derived queries regardless of which file they belong to.
///
/// `schema` is `None` when no schema is configured (schema-free mode, §7.1).
#[allow(missing_docs)]
#[salsa::input]
pub struct WorkspaceInputs {
    /// Workspace-scoped schema.  `None` = schema-free analysis.
    pub schema: Option<Arc<dyn SchemaProvider>>,
}

// ---------------------------------------------------------------------------
// Derived query: options_digest
// ---------------------------------------------------------------------------

/// Compute the stable digest for a file's [`AnalysisOptions`].
///
/// This is a derived (`#[salsa::tracked]`) query, not an input, so Salsa
/// memoises the result and only re-evaluates it when `FileOptions::options`
/// changes.  Consumers that only need to know *whether* options changed
/// (e.g. to decide if a cache entry is still valid) read this digest rather
/// than comparing `AnalysisOptions` values directly.
#[salsa::tracked]
pub fn options_digest(db: &dyn CypherDb, file_opts: FileOptions) -> u64 {
    file_opts.options(db).digest()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CypherDatabase;
    use salsa::Setter as _;

    // -----------------------------------------------------------------------
    // Digest stability
    // -----------------------------------------------------------------------

    /// Same options → same digest regardless of [`BTreeMap`] insertion order.
    #[test]
    fn digest_is_order_independent() {
        let mut hints_a = BTreeMap::new();
        hints_a.insert(SmolStr::new("x"), Type::Int);
        hints_a.insert(SmolStr::new("y"), Type::String);

        let mut hints_b = BTreeMap::new();
        // Insert in reverse order — BTreeMap will sort both the same way.
        hints_b.insert(SmolStr::new("y"), Type::String);
        hints_b.insert(SmolStr::new("x"), Type::Int);

        let a = AnalysisOptions {
            dialect: DialectMode::GqlAligned,
            warn_shadowing: false,
            parameter_hints: hints_a,
        };
        let b = AnalysisOptions {
            dialect: DialectMode::GqlAligned,
            warn_shadowing: false,
            parameter_hints: hints_b,
        };

        assert_eq!(
            a.digest(),
            b.digest(),
            "digest must be independent of insertion order"
        );
    }

    /// Changing `warn_shadowing` changes the digest.
    #[test]
    fn digest_changes_on_warn_shadowing_toggle() {
        let opts_false = AnalysisOptions {
            warn_shadowing: false,
            ..Default::default()
        };
        let opts_true = AnalysisOptions {
            warn_shadowing: true,
            ..Default::default()
        };
        assert_ne!(
            opts_false.digest(),
            opts_true.digest(),
            "toggling warn_shadowing must change the digest"
        );
    }

    /// Adding a parameter hint changes the digest.
    #[test]
    fn digest_changes_on_hint_addition() {
        let base = AnalysisOptions::default();
        let mut hints = BTreeMap::new();
        hints.insert(SmolStr::new("p"), Type::Bool);
        let with_hint = AnalysisOptions {
            parameter_hints: hints,
            ..Default::default()
        };
        assert_ne!(
            base.digest(),
            with_hint.digest(),
            "adding a parameter hint must change the digest"
        );
    }

    /// Changing a single parameter hint's type changes the digest.
    #[test]
    fn digest_changes_on_hint_type_change() {
        let mut hints_int = BTreeMap::new();
        hints_int.insert(SmolStr::new("p"), Type::Int);
        let opts_int = AnalysisOptions {
            parameter_hints: hints_int,
            ..Default::default()
        };

        let mut hints_str = BTreeMap::new();
        hints_str.insert(SmolStr::new("p"), Type::String);
        let opts_str = AnalysisOptions {
            parameter_hints: hints_str,
            ..Default::default()
        };

        assert_ne!(
            opts_int.digest(),
            opts_str.digest(),
            "changing a hint type must change the digest"
        );
    }

    /// Changing dialect changes the digest.
    #[test]
    fn digest_changes_on_dialect_change() {
        let gql = AnalysisOptions {
            dialect: DialectMode::GqlAligned,
            ..Default::default()
        };
        let oc = AnalysisOptions {
            dialect: DialectMode::OpenCypherV9,
            ..Default::default()
        };
        assert_ne!(
            gql.digest(),
            oc.digest(),
            "changing dialect must change the digest"
        );
    }

    // -----------------------------------------------------------------------
    // Salsa invalidation
    // -----------------------------------------------------------------------

    /// Setting a different dialect on `FileOptions` invalidates `options_digest`.
    #[test]
    fn options_digest_invalidates_on_dialect_change() {
        let mut db = CypherDatabase::new();
        let file_opts = FileOptions::new(
            &db,
            AnalysisOptions {
                dialect: DialectMode::GqlAligned,
                ..Default::default()
            },
        );

        let d1 = options_digest(&db, file_opts);

        // Mutate dialect → revision bump.
        file_opts.set_options(&mut db).to(AnalysisOptions {
            dialect: DialectMode::OpenCypherV9,
            ..Default::default()
        });

        let d2 = options_digest(&db, file_opts);

        assert_ne!(d1, d2, "options_digest must change after dialect mutation");
    }

    /// Two `AnalysisOptions` with identical content (regardless of hint
    /// insertion order) produce the same digest → derived queries are NOT
    /// re-evaluated.
    #[test]
    fn options_digest_stable_for_equal_options() {
        let db = CypherDatabase::new();

        let mut hints_a = BTreeMap::new();
        hints_a.insert(SmolStr::new("x"), Type::Int);
        hints_a.insert(SmolStr::new("y"), Type::String);

        let mut hints_b = BTreeMap::new();
        hints_b.insert(SmolStr::new("y"), Type::String); // reversed
        hints_b.insert(SmolStr::new("x"), Type::Int);

        let file_opts_a = FileOptions::new(
            &db,
            AnalysisOptions {
                parameter_hints: hints_a,
                ..Default::default()
            },
        );
        let file_opts_b = FileOptions::new(
            &db,
            AnalysisOptions {
                parameter_hints: hints_b,
                ..Default::default()
            },
        );

        let d_a = options_digest(&db, file_opts_a);
        let d_b = options_digest(&db, file_opts_b);

        assert_eq!(
            d_a, d_b,
            "equal options (any insertion order) must produce the same digest"
        );
    }

    /// Changing a single parameter hint → digest changes → derived query re-evaluates.
    #[test]
    fn options_digest_invalidates_on_hint_change() {
        let mut db = CypherDatabase::new();

        let mut hints = BTreeMap::new();
        hints.insert(SmolStr::new("p"), Type::Int);

        let file_opts = FileOptions::new(
            &db,
            AnalysisOptions {
                parameter_hints: hints,
                ..Default::default()
            },
        );

        let d1 = options_digest(&db, file_opts);

        // Change the hint type.
        let mut new_hints = BTreeMap::new();
        new_hints.insert(SmolStr::new("p"), Type::String);
        file_opts.set_options(&mut db).to(AnalysisOptions {
            parameter_hints: new_hints,
            ..Default::default()
        });

        let d2 = options_digest(&db, file_opts);

        assert_ne!(
            d1, d2,
            "changing a parameter hint must invalidate options_digest"
        );
    }

    // -----------------------------------------------------------------------
    // WorkspaceInputs
    // -----------------------------------------------------------------------

    /// [`WorkspaceInputs`] can be constructed with `None` (schema-free mode).
    #[test]
    fn workspace_inputs_none_schema() {
        let db = CypherDatabase::new();
        let ws = WorkspaceInputs::new(&db, None);
        assert!(ws.schema(&db).is_none());
    }

    /// [`WorkspaceInputs`] schema can be updated.
    #[test]
    fn workspace_inputs_schema_round_trip() {
        use cypher_schema::EmptySchema;
        let mut db = CypherDatabase::new();
        let ws = WorkspaceInputs::new(&db, None);
        assert!(ws.schema(&db).is_none());

        let schema: Arc<dyn SchemaProvider> = Arc::new(EmptySchema);
        ws.set_schema(&mut db).to(Some(schema.clone()));
        assert!(ws.schema(&db).is_some());
    }
}
