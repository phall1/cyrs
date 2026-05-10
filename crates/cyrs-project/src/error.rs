//! Error taxonomy for the project-manifest loader (spec 0003 §7).

use std::path::PathBuf;

use cyrs_schema::file::SchemaLoadError;
use smol_str::SmolStr;
use thiserror::Error;

/// Errors surfaced by the `cypher-project.toml` loader.
///
/// The variant set is closed at v0. See spec 0003 §7.
#[derive(Debug, Error)]
pub enum ProjectLoadError {
    /// Malformed TOML, missing required field, or unknown key.
    #[error("malformed cypher-project.toml: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// Filesystem read or walk failure.
    #[error("io error at {path}: {source}")]
    Io {
        /// Path the loader attempted to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The referenced `schema.toml` failed to load (spec 0002 §11).
    #[error("loading schema at {path}: {source}")]
    Schema {
        /// Path the loader resolved for the schema file.
        path: PathBuf,
        /// Underlying schema-load error.
        #[source]
        source: SchemaLoadError,
    },

    /// A glob string in `include`, `exclude`, or `per_file` is malformed.
    #[error("malformed glob `{pattern}`: {source}")]
    GlobError {
        /// The offending pattern.
        pattern: String,
        /// Underlying globset error.
        #[source]
        source: globset::Error,
    },

    /// The dialect string is not one of `GqlAligned` / `OpenCypherV9`.
    #[error("unknown dialect `{0}`; expected `GqlAligned` or `OpenCypherV9`")]
    UnknownDialect(SmolStr),

    /// A lint rule name appears in `[project.lint]` that is not in the
    /// v0 registry (see [`crate::REGISTERED_LINT_RULES`]).
    #[error("unknown lint rule `{0}`")]
    UnknownLintRule(SmolStr),

    /// `load_from_toml_path` was called on a missing file, or `discover`
    /// returned `None` where a manifest was required.
    #[error("cypher-project.toml not found at {0}")]
    ManifestNotFound(PathBuf),
}
