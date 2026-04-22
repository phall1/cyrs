//! `cypher-project.toml` shape, loader, and round-trip serialiser
//! (spec 0003 §4, §5, §12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cypher_schema::InMemorySchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::error::ProjectLoadError;
use crate::lint::{LintLevel, is_registered_lint_rule};
use crate::resolve::{build_globset, expand_members};

// ============================================================
// Public types
// ============================================================

/// The default dialect declared in `[project.dialect].default`
/// (spec 0003 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DialectDefault {
    /// GQL-aligned Cypher — the v1 default (spec 0001 §9).
    GqlAligned,
    /// openCypher v9 — the legacy / compatibility dialect (spec 0001 §9).
    OpenCypherV9,
}

impl DialectDefault {
    fn parse(s: &str) -> Result<Self, ProjectLoadError> {
        match s {
            "GqlAligned" => Ok(Self::GqlAligned),
            "OpenCypherV9" => Ok(Self::OpenCypherV9),
            other => Err(ProjectLoadError::UnknownDialect(SmolStr::new(other))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::GqlAligned => "GqlAligned",
            Self::OpenCypherV9 => "OpenCypherV9",
        }
    }
}

/// A single entry in `[project.dialect.per_file]`: a glob pattern paired
/// with the dialect applied to matching files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectPerFile {
    /// The raw glob pattern, preserved for diagnostics.
    pub pattern: String,
    /// The dialect applied to files matching the pattern.
    pub dialect: DialectDefault,
}

/// Resolved dialect configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectConfig {
    /// Dialect applied to any member not matched by `per_file`.
    pub default: DialectDefault,
    /// Per-glob dialect overrides, in declaration order.
    pub per_file: Vec<DialectPerFile>,
}

impl Default for DialectConfig {
    fn default() -> Self {
        Self {
            default: DialectDefault::GqlAligned,
            per_file: Vec::new(),
        }
    }
}

/// The resolved, validated manifest returned by the loader
/// (spec 0003 §12).
#[derive(Debug, Clone)]
pub struct ProjectManifest {
    /// Project name (`[project].name`).
    pub name: SmolStr,
    /// Optional semver string (`[project].version`).
    pub version: Option<SmolStr>,
    /// Optional free-form description (`[project].description`).
    pub description: Option<String>,
    /// Dialect defaults and per-file overrides.
    pub dialect: DialectConfig,
    /// Resolved member files, absolute paths, sorted.
    pub members: Vec<PathBuf>,
    /// Raw include globs, preserved for diagnostics.
    pub include: Vec<String>,
    /// Raw exclude globs, preserved for diagnostics.
    pub exclude: Vec<String>,
    /// Loaded schema, if `[project.schema].path` was set.
    pub schema: Option<InMemorySchema>,
    /// Raw schema path (relative to manifest dir), preserved for
    /// round-tripping even when the schema failed to load.
    pub schema_path: Option<PathBuf>,
    /// Resolved lint levels, keyed by rule name.
    pub lint_levels: BTreeMap<String, LintLevel>,
    /// Directory containing the manifest file. Absolute when loaded via
    /// [`load_from_toml_path`]; the manifest's own directory when loaded
    /// via [`load_from_toml_str`] with a supplied base.
    pub manifest_dir: PathBuf,
}

// ============================================================
// Serde-shaped wire format
// ============================================================

/// The raw `cypher-project.toml` file shape.
///
/// Public so callers can inspect or rewrite the file before / after the
/// conversion into [`ProjectManifest`]. See spec 0003 §4.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    /// The single top-level `[project]` table (required).
    pub project: ProjectTable,
}

/// `[project]` table contents (spec 0003 §4.1-§4.5).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTable {
    /// Project name.
    pub name: String,
    /// Optional semver string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `[project.dialect]` sub-table.
    #[serde(default, skip_serializing_if = "DialectTable::is_default")]
    pub dialect: DialectTable,
    /// `[project.members]` sub-table.
    #[serde(default, skip_serializing_if = "MembersTable::is_default")]
    pub members: MembersTable,
    /// `[project.schema]` sub-table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<SchemaTable>,
    /// `[project.lint]` map of rule → level.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lint: BTreeMap<String, LintLevel>,
}

/// `[project.dialect]` sub-table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialectTable {
    /// Default dialect string. `None` ⇒ `GqlAligned`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Optional per-glob overrides.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_file: BTreeMap<String, String>,
}

impl DialectTable {
    fn is_default(&self) -> bool {
        self.default.is_none() && self.per_file.is_empty()
    }
}

/// `[project.members]` sub-table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembersTable {
    /// Include globs (default: `["**/*.cyp"]`).
    #[serde(default = "default_include")]
    pub include: Vec<String>,
    /// Exclude globs (default: `["**/target/**", "**/.git/**"]`).
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
}

impl Default for MembersTable {
    fn default() -> Self {
        Self {
            include: default_include(),
            exclude: default_exclude(),
        }
    }
}

impl MembersTable {
    fn is_default(&self) -> bool {
        self.include == default_include() && self.exclude == default_exclude()
    }
}

fn default_include() -> Vec<String> {
    vec!["**/*.cyp".to_owned()]
}

fn default_exclude() -> Vec<String> {
    vec!["**/target/**".to_owned(), "**/.git/**".to_owned()]
}

/// `[project.schema]` sub-table (spec 0003 §4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaTable {
    /// Schema path, relative to the manifest directory.
    pub path: String,
}

// ============================================================
// Public API
// ============================================================

/// Parse a `cypher-project.toml` string.
///
/// - `input` — the raw TOML text.
/// - `manifest_dir` — the directory the manifest's relative paths are
///   resolved against (e.g., the schema path, member globs). When
///   `None` the current working directory is used. Most callers should
///   prefer [`load_from_toml_path`], which supplies the right directory
///   automatically.
pub fn load_from_toml_str(
    input: &str,
    manifest_dir: Option<&Path>,
) -> Result<ProjectManifest, ProjectLoadError> {
    let file: ProjectFile = toml::from_str(input)?;
    let dir = match manifest_dir {
        Some(d) => d.to_path_buf(),
        None => std::env::current_dir().map_err(|source| ProjectLoadError::Io {
            path: PathBuf::from("."),
            source,
        })?,
    };
    file.into_manifest(&dir)
}

/// Read the file at `path` and parse it as a `cypher-project.toml`.
///
/// The manifest's relative paths are resolved against `path`'s parent
/// directory.
pub fn load_from_toml_path(path: &Path) -> Result<ProjectManifest, ProjectLoadError> {
    if !path.is_file() {
        return Err(ProjectLoadError::ManifestNotFound(path.to_path_buf()));
    }
    let input = std::fs::read_to_string(path).map_err(|source| ProjectLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    load_from_toml_str(&input, Some(&dir))
}

// ============================================================
// Conversion
// ============================================================

impl ProjectFile {
    /// Convert into a [`ProjectManifest`], expanding members, loading
    /// the schema (if any), and validating lint rule names.
    pub fn into_manifest(self, manifest_dir: &Path) -> Result<ProjectManifest, ProjectLoadError> {
        let ProjectTable {
            name,
            version,
            description,
            dialect,
            members,
            schema,
            lint,
        } = self.project;

        // --- dialect ---
        let default_dialect = match dialect.default.as_deref() {
            None => DialectDefault::GqlAligned,
            Some(s) => DialectDefault::parse(s)?,
        };
        let mut per_file = Vec::with_capacity(dialect.per_file.len());
        for (pattern, dstr) in dialect.per_file {
            // Validate the pattern compiles; surface early if not.
            build_globset(std::slice::from_ref(&pattern))?;
            per_file.push(DialectPerFile {
                pattern,
                dialect: DialectDefault::parse(&dstr)?,
            });
        }
        let dialect = DialectConfig {
            default: default_dialect,
            per_file,
        };

        // --- members (glob expansion against manifest_dir) ---
        let resolved_members = expand_members(manifest_dir, &members.include, &members.exclude)?;

        // --- schema (optional; spec 0002 via cypher-schema::file) ---
        let (schema_obj, schema_path) = match schema {
            None => (None, None),
            Some(SchemaTable { path }) => {
                let abs = manifest_dir.join(&path);
                let loaded = cypher_schema::file::load_from_toml_path(&abs).map_err(|source| {
                    ProjectLoadError::Schema {
                        path: abs.clone(),
                        source,
                    }
                })?;
                (Some(loaded), Some(PathBuf::from(path)))
            }
        };

        // --- lint (validate rule names against §6 registry) ---
        let mut lint_levels: BTreeMap<String, LintLevel> = BTreeMap::new();
        for (rule, level) in lint {
            if !is_registered_lint_rule(&rule) {
                return Err(ProjectLoadError::UnknownLintRule(SmolStr::new(&rule)));
            }
            lint_levels.insert(rule, level);
        }

        Ok(ProjectManifest {
            name: SmolStr::new(&name),
            version: version.map(SmolStr::from),
            description,
            dialect,
            members: resolved_members,
            include: members.include,
            exclude: members.exclude,
            schema: schema_obj,
            schema_path,
            lint_levels,
            manifest_dir: manifest_dir.to_path_buf(),
        })
    }
}

impl ProjectFile {
    /// Build a [`ProjectFile`] view of a [`ProjectManifest`], suitable
    /// for serialisation back to TOML. The round-trip is semantic, not
    /// byte-for-byte: ordering within collections is canonicalised.
    #[must_use]
    pub fn from_manifest(m: &ProjectManifest) -> Self {
        let default_dialect_str = match m.dialect.default {
            DialectDefault::GqlAligned => None,
            DialectDefault::OpenCypherV9 => Some(DialectDefault::OpenCypherV9.as_str().to_owned()),
        };
        let per_file: BTreeMap<String, String> = m
            .dialect
            .per_file
            .iter()
            .map(|d| (d.pattern.clone(), d.dialect.as_str().to_owned()))
            .collect();

        let members = MembersTable {
            include: m.include.clone(),
            exclude: m.exclude.clone(),
        };

        let schema = m.schema_path.as_ref().map(|p| SchemaTable {
            path: p.to_string_lossy().into_owned(),
        });

        Self {
            project: ProjectTable {
                name: m.name.to_string(),
                version: m.version.as_ref().map(ToString::to_string),
                description: m.description.clone(),
                dialect: DialectTable {
                    default: default_dialect_str,
                    per_file,
                },
                members,
                schema,
                lint: m.lint_levels.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_manifest_parses() {
        let toml = r#"
[project]
name = "demo"
"#;
        let tmp = tempfile::tempdir().unwrap();
        let m = load_from_toml_str(toml, Some(tmp.path())).unwrap();
        assert_eq!(m.name, "demo");
        assert_eq!(m.dialect.default, DialectDefault::GqlAligned);
        assert!(m.members.is_empty());
        assert!(m.schema.is_none());
        assert!(m.lint_levels.is_empty());
    }

    #[test]
    fn unknown_dialect_rejected() {
        let toml = r#"
[project]
name = "demo"

[project.dialect]
default = "Cypher25"
"#;
        let tmp = tempfile::tempdir().unwrap();
        let err = load_from_toml_str(toml, Some(tmp.path())).unwrap_err();
        assert!(matches!(err, ProjectLoadError::UnknownDialect(_)));
    }

    #[test]
    fn unknown_lint_rule_rejected() {
        let toml = r#"
[project]
name = "demo"

[project.lint]
"no-such-rule" = "warn"
"#;
        let tmp = tempfile::tempdir().unwrap();
        let err = load_from_toml_str(toml, Some(tmp.path())).unwrap_err();
        assert!(matches!(err, ProjectLoadError::UnknownLintRule(_)));
    }

    #[test]
    fn unknown_top_level_key_rejected() {
        let toml = r#"
[project]
name = "demo"

[workspace]
oops = true
"#;
        let tmp = tempfile::tempdir().unwrap();
        let err = load_from_toml_str(toml, Some(tmp.path())).unwrap_err();
        assert!(matches!(err, ProjectLoadError::TomlParse(_)));
    }
}
