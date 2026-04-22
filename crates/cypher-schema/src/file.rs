//! TOML schema-file loader and round-trip serialiser (spec 0002).
//!
//! Gated by the `file` feature so the default build does not pull in
//! `toml` or `thiserror`. The public API is three functions:
//!
//! - [`load_from_toml_str`] — parse a TOML string.
//! - [`load_from_toml_path`] — read a file and parse its contents.
//! - [`serialise_to_toml`] — render an [`InMemorySchema`] back to TOML
//!   for the round-trip property in spec 0002 §10.
//!
//! Intermediate serde-shaped structs ([`SchemaFile`], [`MetaBlock`],
//! [`LabelEntry`], [`RelTypeEntry`], [`ParameterEntry`],
//! [`PropertyEntry`]) convert to/from [`InMemorySchema`]. They are
//! public so callers who want to introspect or massage the raw file
//! shape before conversion can do so.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use thiserror::Error;

use crate::{
    InMemorySchema, ParamDecl, PropertyDecl, PropertyType, RelDecl,
    in_memory::{BuilderError, InMemorySchemaBuilder},
};

/// Format version accepted by this loader. See spec 0002 §8.
pub const SCHEMA_FILE_VERSION: &str = "0.1.0";

// ============================================================
// Public API
// ============================================================

/// Parse a `schema.toml` string into an [`InMemorySchema`].
pub fn load_from_toml_str(input: &str) -> Result<InMemorySchema, SchemaLoadError> {
    let file: SchemaFile = toml::from_str(input)?;
    file.into_schema()
}

/// Read the file at `path` and parse it as a `schema.toml`.
pub fn load_from_toml_path(path: &Path) -> Result<InMemorySchema, SchemaLoadError> {
    let input = std::fs::read_to_string(path).map_err(|source| SchemaLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_from_toml_str(&input)
}

/// Render an [`InMemorySchema`] back to a TOML string.
///
/// The output satisfies the spec 0002 §10 round-trip property: feeding
/// it back through [`load_from_toml_str`] yields a schema semantically
/// equal to the input (collection ordering uses `BTreeMap` internally
/// so the result is deterministic).
#[must_use]
pub fn serialise_to_toml(schema: &InMemorySchema) -> String {
    let file = SchemaFile::from_schema(schema);
    toml::to_string_pretty(&file).expect("SchemaFile serialises infallibly")
}

// ============================================================
// Error taxonomy
// ============================================================

/// Errors surfaced by the TOML schema loader (spec 0002 §11).
#[derive(Debug, Error)]
pub enum SchemaLoadError {
    /// The input is not well-formed TOML, or does not match the
    /// expected shape.
    #[error("malformed schema TOML: {0}")]
    TomlParse(#[from] toml::de::Error),
    /// Reading the file failed.
    #[error("reading schema from {path}: {source}")]
    Io {
        /// Path we tried to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A relationship type references an undeclared label.
    #[error("rel type endpoint references unknown label `{0}`")]
    UnknownLabelRef(SmolStr),
    /// A label name is declared twice.
    #[error("duplicate label `{0}`")]
    DuplicateLabel(SmolStr),
    /// A rel type name is declared twice.
    #[error("duplicate rel type `{0}`")]
    DuplicateRelType(SmolStr),
    /// A parameter name is declared twice.
    #[error("duplicate parameter `{0}`")]
    DuplicateParameter(SmolStr),
    /// A `type` string lies outside the grammar in spec 0002 §4, or
    /// `[meta].cyrs_schema_version` does not match the supported version.
    #[error("bad type string: {0}")]
    BadType(String),
}

impl From<BuilderError> for SchemaLoadError {
    fn from(err: BuilderError) -> Self {
        match err {
            BuilderError::DuplicateLabel(n) => Self::DuplicateLabel(n),
            BuilderError::DuplicateRelType(n) => Self::DuplicateRelType(n),
            BuilderError::DuplicateParameter(n) => Self::DuplicateParameter(n),
        }
    }
}

// ============================================================
// serde-shaped intermediate structs
// ============================================================

/// Top-level shape of a `schema.toml` file (spec 0002 §3).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaFile {
    /// Optional `[meta]` block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaBlock>,
    /// `[[label]]` array of tables.
    #[serde(default, rename = "label", skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelEntry>,
    /// `[[rel_type]]` array of tables.
    #[serde(default, rename = "rel_type", skip_serializing_if = "Vec::is_empty")]
    pub rel_types: Vec<RelTypeEntry>,
    /// `[[parameter]]` array of tables.
    #[serde(default, rename = "parameter", skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParameterEntry>,
}

/// `[meta]` block contents (spec 0002 §8).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetaBlock {
    /// Format version; must equal [`SCHEMA_FILE_VERSION`].
    pub cyrs_schema_version: String,
    /// Optional human-friendly name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `[[label]]` entry (spec 0002 §5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelEntry {
    /// Label name.
    pub name: String,
    /// Declared properties.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<PropertyEntry>,
}

/// `[[rel_type]]` entry (spec 0002 §6).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelTypeEntry {
    /// Rel type name.
    pub name: String,
    /// Allowed start-endpoint labels. Empty = polymorphic.
    #[serde(default)]
    pub start_labels: Vec<String>,
    /// Allowed end-endpoint labels. Empty = polymorphic.
    #[serde(default)]
    pub end_labels: Vec<String>,
    /// Properties declared on the relationship.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<PropertyEntry>,
}

/// `[[parameter]]` entry (spec 0002 §7).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterEntry {
    /// Parameter name, without the leading `$`.
    pub name: String,
    /// Declared type; must satisfy the §4 grammar.
    #[serde(rename = "type")]
    pub ty: String,
    /// Optional scalar default (string / int / float / bool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<toml::Value>,
}

/// Property entry used inside label and rel type declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyEntry {
    /// Property name.
    pub name: String,
    /// Declared type; must satisfy the §4 grammar.
    #[serde(rename = "type")]
    pub ty: String,
    /// `true` when the property is required on every instance.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
}

#[inline]
#[allow(clippy::trivially_copy_pass_by_ref)] // serde requires `&T`.
fn is_false(b: &bool) -> bool {
    !*b
}

// ============================================================
// Conversion
// ============================================================

impl SchemaFile {
    /// Convert into an [`InMemorySchema`], validating invariants along
    /// the way.
    pub fn into_schema(self) -> Result<InMemorySchema, SchemaLoadError> {
        if let Some(meta) = &self.meta
            && meta.cyrs_schema_version != SCHEMA_FILE_VERSION
        {
            return Err(SchemaLoadError::BadType(format!(
                "unsupported cyrs_schema_version `{}`; this loader speaks `{SCHEMA_FILE_VERSION}`",
                meta.cyrs_schema_version
            )));
        }

        let mut builder = InMemorySchemaBuilder::default();

        // Track declared label names for rel-type endpoint validation.
        let mut declared: BTreeSet<SmolStr> = BTreeSet::new();
        for lbl in &self.labels {
            let key = SmolStr::new(&lbl.name);
            if !declared.insert(key.clone()) {
                return Err(SchemaLoadError::DuplicateLabel(key));
            }
        }

        for lbl in self.labels {
            let name = SmolStr::new(&lbl.name);
            let mut props = Vec::with_capacity(lbl.properties.len());
            for p in lbl.properties {
                props.push(p.into_decl()?);
            }
            builder = builder.add_label(name, props);
        }

        let mut rel_names: BTreeSet<SmolStr> = BTreeSet::new();
        for rel in &self.rel_types {
            let key = SmolStr::new(&rel.name);
            if !rel_names.insert(key.clone()) {
                return Err(SchemaLoadError::DuplicateRelType(key));
            }
            for endpoint in rel.start_labels.iter().chain(rel.end_labels.iter()) {
                let ep = SmolStr::new(endpoint);
                if !declared.contains(&ep) {
                    return Err(SchemaLoadError::UnknownLabelRef(ep));
                }
            }
        }

        for rel in self.rel_types {
            let mut props = Vec::with_capacity(rel.properties.len());
            for p in rel.properties {
                props.push(p.into_decl()?);
            }
            builder = builder.add_rel_type(RelDecl {
                name: SmolStr::new(&rel.name),
                start_labels: rel.start_labels.into_iter().map(SmolStr::from).collect(),
                end_labels: rel.end_labels.into_iter().map(SmolStr::from).collect(),
                properties: props,
            });
        }

        for p in self.parameters {
            builder = builder.add_parameter(p.into_decl()?);
        }

        if let Some(meta) = self.meta {
            builder = builder
                .schema_name(meta.schema_name.map(SmolStr::from))
                .description(meta.description);
        }

        builder.build().map_err(SchemaLoadError::from)
    }

    /// Build a serialisable view of a schema (the inverse of
    /// [`SchemaFile::into_schema`]).
    #[must_use]
    pub fn from_schema(schema: &InMemorySchema) -> Self {
        let meta = Some(MetaBlock {
            cyrs_schema_version: SCHEMA_FILE_VERSION.to_owned(),
            schema_name: schema.schema_name.as_ref().map(ToString::to_string),
            description: schema.description.clone(),
        });

        let labels = schema
            .labels
            .iter()
            .map(|(name, props)| LabelEntry {
                name: name.to_string(),
                properties: props.iter().map(PropertyEntry::from_decl).collect(),
            })
            .collect();

        let rel_types = schema
            .rel_types
            .values()
            .map(|r| RelTypeEntry {
                name: r.name.to_string(),
                start_labels: r.start_labels.iter().map(ToString::to_string).collect(),
                end_labels: r.end_labels.iter().map(ToString::to_string).collect(),
                properties: r.properties.iter().map(PropertyEntry::from_decl).collect(),
            })
            .collect();

        let parameters = schema
            .parameters
            .values()
            .map(ParameterEntry::from_decl)
            .collect();

        Self {
            meta,
            labels,
            rel_types,
            parameters,
        }
    }
}

impl PropertyEntry {
    fn into_decl(self) -> Result<PropertyDecl, SchemaLoadError> {
        Ok(PropertyDecl {
            name: SmolStr::new(&self.name),
            ty: parse_type(&self.ty)?,
            required: self.required,
        })
    }

    fn from_decl(d: &PropertyDecl) -> Self {
        Self {
            name: d.name.to_string(),
            ty: render_type(&d.ty),
            required: d.required,
        }
    }
}

impl ParameterEntry {
    fn into_decl(self) -> Result<ParamDecl, SchemaLoadError> {
        let default = match self.default {
            None => None,
            Some(v) => Some(render_default_literal(&v)?),
        };
        Ok(ParamDecl {
            name: SmolStr::new(&self.name),
            ty: parse_type(&self.ty)?,
            default,
        })
    }

    fn from_decl(d: &ParamDecl) -> Self {
        Self {
            name: d.name.to_string(),
            ty: render_type(&d.ty),
            default: d.default.as_ref().map(parse_default_literal),
        }
    }
}

// ============================================================
// Type grammar (spec 0002 §4)
// ============================================================

fn parse_type(input: &str) -> Result<PropertyType, SchemaLoadError> {
    let trimmed = input.trim();
    // NULLABLE is a modifier; at v0 we parse it transparently — the
    // PropertyType lattice in spec 0001 §8.2 has no NULLABLE variant,
    // so we accept the modifier and surface the underlying type.
    if let Some(rest) = trimmed.strip_prefix("NULLABLE ") {
        return parse_type(rest);
    }
    if let Some(inner) = trimmed
        .strip_prefix("LIST<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return Ok(PropertyType::List(Box::new(parse_type(inner)?)));
    }
    Ok(match trimmed {
        "STRING" => PropertyType::String,
        "INTEGER" => PropertyType::Int,
        "FLOAT" => PropertyType::Float,
        "BOOLEAN" => PropertyType::Bool,
        "DATE" => PropertyType::Date,
        "DATETIME" => PropertyType::Datetime,
        "DURATION" => PropertyType::Opaque(SmolStr::new("DURATION")),
        "POINT" => PropertyType::Opaque(SmolStr::new("POINT")),
        "MAP" => PropertyType::Opaque(SmolStr::new("MAP")),
        "NULL" => PropertyType::Opaque(SmolStr::new("NULL")),
        other => return Err(SchemaLoadError::BadType(other.to_owned())),
    })
}

fn render_type(ty: &PropertyType) -> String {
    match ty {
        PropertyType::String => "STRING".to_owned(),
        PropertyType::Int => "INTEGER".to_owned(),
        PropertyType::Float => "FLOAT".to_owned(),
        PropertyType::Bool => "BOOLEAN".to_owned(),
        PropertyType::Date => "DATE".to_owned(),
        PropertyType::Datetime => "DATETIME".to_owned(),
        PropertyType::List(inner) => format!("LIST<{}>", render_type(inner)),
        // Enum / Any are not expressible in the v0 file format (spec
        // 0002 §20). Render Enum with its type name and Any as MAP so
        // round-trip still produces valid TOML. A later spec may add
        // richer syntax.
        PropertyType::Opaque(n) | PropertyType::Enum(n, _) => n.to_string(),
        PropertyType::Any => "MAP".to_owned(),
    }
}

fn parse_default_literal(s: &SmolStr) -> toml::Value {
    if let Ok(b) = s.parse::<bool>() {
        return toml::Value::Boolean(b);
    }
    if let Ok(i) = s.parse::<i64>() {
        return toml::Value::Integer(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return toml::Value::Float(f);
    }
    // Fall back to a string; strip surrounding quotes if present so
    // the round-trip does not accumulate escaping.
    let stripped = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s.as_str());
    toml::Value::String(stripped.to_owned())
}

fn render_default_literal(v: &toml::Value) -> Result<SmolStr, SchemaLoadError> {
    Ok(match v {
        toml::Value::String(s) => SmolStr::new(s),
        toml::Value::Integer(i) => SmolStr::new(i.to_string()),
        toml::Value::Float(f) => SmolStr::new(f.to_string()),
        toml::Value::Boolean(b) => SmolStr::new(b.to_string()),
        other => {
            return Err(SchemaLoadError::BadType(format!(
                "parameter default must be a scalar (string, integer, float, boolean); got {other}",
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_type_primitives() {
        assert_eq!(parse_type("STRING").unwrap(), PropertyType::String);
        assert_eq!(parse_type("INTEGER").unwrap(), PropertyType::Int);
        assert_eq!(
            parse_type("LIST<STRING>").unwrap(),
            PropertyType::List(Box::new(PropertyType::String))
        );
        assert_eq!(parse_type("NULLABLE STRING").unwrap(), PropertyType::String);
        assert_eq!(
            parse_type("LIST<NULLABLE INTEGER>").unwrap(),
            PropertyType::List(Box::new(PropertyType::Int))
        );
    }

    #[test]
    fn parse_type_rejects_garbage() {
        let err = parse_type("not a type").unwrap_err();
        assert!(matches!(err, SchemaLoadError::BadType(_)));
    }

    #[test]
    fn render_type_round_trips_primitives() {
        for (s, _t) in [
            ("STRING", PropertyType::String),
            ("INTEGER", PropertyType::Int),
            ("FLOAT", PropertyType::Float),
            ("BOOLEAN", PropertyType::Bool),
            ("DATE", PropertyType::Date),
            ("DATETIME", PropertyType::Datetime),
        ] {
            let t = parse_type(s).unwrap();
            assert_eq!(render_type(&t), s);
        }
        let nested = parse_type("LIST<LIST<INTEGER>>").unwrap();
        assert_eq!(render_type(&nested), "LIST<LIST<INTEGER>>");
    }
}
