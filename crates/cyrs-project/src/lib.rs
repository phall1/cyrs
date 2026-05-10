//! `cyrs-project` — `cypher-project.toml` manifest format and loader (spec 0003).
//!
//! A Cypher / GQL project is a directory rooted by a `cypher-project.toml`
//! file that declares:
//!
//! - the project's identity (`name`, `version`, `description`);
//! - the default dialect (`GqlAligned` or `OpenCypherV9`) with optional
//!   per-glob overrides;
//! - the set of member `.cyp` files (via `include` / `exclude` globs);
//! - an optional schema (`schema.toml` per spec 0002) shared by every
//!   member;
//! - project-local lint levels mapped from a small registry of rule names.
//!
//! Discovery walks up the directory tree from a starting path, exactly as
//! Cargo discovers `Cargo.toml`. Loading parses the file, expands the
//! member globs, loads the referenced schema, and validates lint rule
//! names against the §6 registry. Cross-file analysis is **out of scope
//! at v0** — see the parent epic `cy-o8c` and spec 0003 §20.
//!
//! # Example
//!
//! ```
//! # use cyrs_project::{load_from_toml_str, DialectDefault};
//! let toml = r#"
//! [project]
//! name = "demo"
//!
//! [project.dialect]
//! default = "GqlAligned"
//! "#;
//! let manifest = load_from_toml_str(toml, None).expect("valid manifest");
//! assert_eq!(manifest.name, "demo");
//! assert_eq!(manifest.dialect.default, DialectDefault::GqlAligned);
//! ```

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cyrs-project/0.0.1")]

mod error;
mod lint;
mod manifest;
mod resolve;

pub use error::ProjectLoadError;
pub use lint::{LintLevel, REGISTERED_LINT_RULES, is_registered_lint_rule};
pub use manifest::{
    DialectConfig, DialectDefault, DialectPerFile, ProjectFile, ProjectManifest,
    load_from_toml_path, load_from_toml_str,
};
pub use resolve::discover;

/// The file name the loader discovers when walking up the directory
/// tree. See spec 0003 §2.
pub const MANIFEST_FILE_NAME: &str = "cypher-project.toml";
