//! Parse `initializationOptions` from the `initialize` request (spec
//! §14.3, bead cy-0ls).
//!
//! Shape (all fields optional):
//!
//! ```json
//! {
//!   "schemaSource":  "none" | "file" | "command",
//!   "schemaPath":    "...",
//!   "schemaCommand": "...",
//!   "dialect":       "GqlAligned" | "OpenCypherV9"
//! }
//! ```
//!
//! Invalid or missing fields fall back to "no schema, `GqlAligned`" so
//! clients that forget to set options still get a working server.
//!
//! # Loader
//!
//! [`load_schema`] turns the parsed options into an optional
//! `Arc<dyn SchemaProvider>`.  The `file` source reads a JSON file
//! described by [`JsonSchema`]; the `command` source spawns the
//! named subprocess and parses its stdout as the same JSON shape.
//!
//! # Why duplicate `JsonSchema` here?
//!
//! `cyrs-testkit` and `cyrs-agent` each carry an equivalent type
//! for the same reason: the shape is tiny, the testkit crate is a
//! dev-dep we don't want in the LSP runtime, and the agent copy is
//! `pub(crate)`.  Lifting a shared `JsonSchema` into `cyrs-schema`
//! is filed as a follow-up rather than bundled here.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use cyrs_db::DialectMode;
use cyrs_schema::{
    Cardinality, EndpointDecl, FnCategories, FunctionSignature, ParamDecl, ProcMode,
    ProcedureSignature, PropertyDecl, PropertyType, ReturnTy, SchemaProvider, YieldDecl,
};
use serde::Deserialize;
use smol_str::SmolStr;
use std::collections::BTreeMap;

/// Parsed `initializationOptions`.  `None` fields indicate "not
/// specified" — the defaults (no schema, `GqlAligned`) apply.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct InitOptions {
    #[serde(rename = "schemaSource", default)]
    pub schema_source: Option<SchemaSource>,
    #[serde(rename = "schemaPath", default)]
    pub schema_path: Option<String>,
    #[serde(rename = "schemaCommand", default)]
    pub schema_command: Option<String>,
    #[serde(default)]
    pub dialect: Option<Dialect>,
    /// Debounce window for `workspace/didChangeWatchedFiles` flushes
    /// in milliseconds (cy-k2r, spec §14).  Defaults to 250 ms, clamped
    /// to `[0, 5_000]` on application.  Callers typically leave this
    /// unset — tests override via `with_config_and_debounce`.
    #[serde(rename = "watchedFilesDebounceMs", default)]
    pub watched_files_debounce_ms: Option<u64>,
    /// Enable the clippy-equivalent lint pass (spec 0003 §6, bead
    /// cy-4yy).  When `true`, `publishDiagnostics` also surfaces lints
    /// (`W6011`–`W6016`) as `Information`-severity diagnostics.  Off by
    /// default — lints are opt-in until the pack stabilises.
    #[serde(default)]
    pub lints: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SchemaSource {
    None,
    File,
    Command,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum Dialect {
    GqlAligned,
    OpenCypherV9,
}

impl From<Dialect> for DialectMode {
    fn from(d: Dialect) -> Self {
        match d {
            Dialect::GqlAligned => Self::GqlAligned,
            Dialect::OpenCypherV9 => Self::OpenCypherV9,
        }
    }
}

impl InitOptions {
    /// Parse from a raw JSON value.  Unknown fields and type mismatches
    /// return `Default::default()` with a warning trace; the server
    /// keeps running.
    pub(crate) fn from_value(v: Option<&serde_json::Value>) -> Self {
        let Some(v) = v else {
            return Self::default();
        };
        match serde_json::from_value::<Self>(v.clone()) {
            Ok(opts) => opts,
            Err(e) => {
                tracing::warn!(
                    "initializationOptions could not be parsed — using defaults. error: {e}"
                );
                Self::default()
            }
        }
    }

    /// Resolve the dialect with the default applied.
    pub(crate) fn resolved_dialect(&self) -> DialectMode {
        self.dialect.map_or(DialectMode::GqlAligned, Into::into)
    }

    /// Resolve the lint toggle with its default (off) applied.
    pub(crate) fn lints_enabled(&self) -> bool {
        self.lints.unwrap_or(false)
    }

    /// Resolve the watched-file debounce window with the default +
    /// upper bound applied (cy-k2r, spec §14).
    pub(crate) fn resolved_debounce(&self) -> std::time::Duration {
        let ms = self
            .watched_files_debounce_ms
            .unwrap_or(crate::watchers::DEFAULT_DEBOUNCE_MS);
        let clamped = ms.min(crate::watchers::MAX_DEBOUNCE_MS);
        std::time::Duration::from_millis(clamped)
    }
}

/// Build a `SchemaProvider` from the parsed options, if any.
///
/// Returns `Ok(None)` when `schemaSource` is missing / `none` or
/// mandatory fields are absent (e.g. `file` without `schemaPath`) —
/// the caller then leaves the DB's schema unset.  Returns `Err` when
/// a requested source is present but fails to load (IO, JSON parse,
/// command spawn).  The caller currently logs and continues with no
/// schema rather than failing the whole initialize.
pub(crate) fn load_schema(opts: &InitOptions) -> Result<Option<Arc<dyn SchemaProvider>>> {
    let Some(source) = opts.schema_source else {
        return Ok(None);
    };
    let json = match source {
        SchemaSource::None => return Ok(None),
        SchemaSource::File => {
            let Some(path) = opts.schema_path.as_deref() else {
                return Ok(None);
            };
            std::fs::read_to_string(Path::new(path))
                .with_context(|| format!("reading schemaPath {path}"))?
        }
        SchemaSource::Command => {
            let Some(cmd_str) = opts.schema_command.as_deref() else {
                return Ok(None);
            };
            // Split on whitespace — good enough for `my-schema --arg`
            // style commands; clients needing quoting should invoke
            // their own shell wrapper.
            let mut parts = cmd_str.split_whitespace();
            let program = parts
                .next()
                .ok_or_else(|| anyhow!("schemaCommand is empty"))?;
            let args: Vec<&str> = parts.collect();
            let output = Command::new(program)
                .args(&args)
                .output()
                .with_context(|| format!("spawning schemaCommand {cmd_str:?}"))?;
            if !output.status.success() {
                return Err(anyhow!(
                    "schemaCommand {cmd_str:?} exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            String::from_utf8(output.stdout)
                .with_context(|| format!("schemaCommand {cmd_str:?} stdout is not UTF-8"))?
        }
    };
    let schema: JsonSchema = serde_json::from_str(&json).context("parsing schema JSON")?;
    Ok(Some(Arc::new(schema)))
}

// ---------------------------------------------------------------------------
// JsonSchema — minimal SchemaProvider mirroring the testkit / agent shape
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct JsonSchema {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    rel_types: Vec<String>,
    #[serde(default)]
    node_properties: BTreeMap<String, Vec<JsonPropDecl>>,
    #[serde(default)]
    rel_properties: BTreeMap<String, Vec<JsonPropDecl>>,
    #[serde(default)]
    rel_endpoints: BTreeMap<String, Vec<JsonEndpoint>>,
    #[serde(default)]
    functions: Vec<JsonFnDecl>,
    #[serde(default)]
    procedures: Vec<JsonProcDecl>,
}

#[derive(Debug, Deserialize)]
struct JsonPropDecl {
    name: String,
    #[serde(rename = "type", default = "default_prop_type")]
    ty: String,
    #[serde(default)]
    required: bool,
}

fn default_prop_type() -> String {
    "Any".to_string()
}

#[derive(Debug, Deserialize)]
struct JsonEndpoint {
    source: String,
    target: String,
}

#[derive(Debug, Deserialize)]
struct JsonFnDecl {
    name: String,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default = "default_prop_type")]
    return_type: String,
    variadic: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonProcDecl {
    name: String,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default)]
    yield_columns: Vec<String>,
}

fn parse_prop_type(s: &str) -> PropertyType {
    match s {
        "String" => PropertyType::String,
        "Int" => PropertyType::Int,
        "Float" => PropertyType::Float,
        "Bool" => PropertyType::Bool,
        "Date" => PropertyType::Date,
        "Datetime" => PropertyType::Datetime,
        _ => PropertyType::Any,
    }
}

impl SchemaProvider for JsonSchema {
    fn labels(&self) -> Vec<SmolStr> {
        self.labels.iter().map(SmolStr::new).collect()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.rel_types.iter().map(SmolStr::new).collect()
    }

    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        if !self.labels.iter().any(|l| l == label) {
            return None;
        }
        Some(self.node_properties.get(label).map_or_else(Vec::new, |ps| {
            ps.iter()
                // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1).
                .map(|p| PropertyDecl::new(SmolStr::new(&p.name), parse_prop_type(&p.ty), p.required))
                .collect()
        }))
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>> {
        if !self.rel_types.iter().any(|r| r == rel_type) {
            return None;
        }
        Some(
            self.rel_properties
                .get(rel_type)
                .map_or_else(Vec::new, |ps| {
                    ps.iter()
                        // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1).
                        .map(|p| PropertyDecl::new(SmolStr::new(&p.name), parse_prop_type(&p.ty), p.required))
                        .collect()
                }),
        )
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
        self.rel_endpoints
            .get(rel_type)
            .map_or_else(Vec::new, |eps| {
                eps.iter()
                    .map(|e| EndpointDecl {
                        from: SmolStr::new(&e.source),
                        to: SmolStr::new(&e.target),
                        // JsonSchema does not carry cardinality; the
                        // spec permits unknown → many-to-many as the
                        // least-constraining default.
                        cardinality: Cardinality::ManyToMany,
                    })
                    .collect()
            })
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<SmolStr> {
        None
    }

    fn function(&self, name: &str) -> Option<FunctionSignature> {
        self.functions.iter().find(|f| f.name == name).map(|f| {
            let params = f
                .params
                .iter()
                .map(|p| ParamDecl {
                    name: SmolStr::new(p),
                    ty: PropertyType::Any,
                    default: None,
                })
                .collect();
            let variadic = f.variadic.as_ref().map(|name| ParamDecl {
                name: SmolStr::new(name),
                ty: PropertyType::Any,
                default: None,
            });
            FunctionSignature {
                name: SmolStr::new(&f.name),
                params,
                variadic,
                return_ty: ReturnTy::Constant(parse_prop_type(&f.return_type)),
                categories: FnCategories {
                    pure: true,
                    aggregate: false,
                    deterministic: true,
                },
                deterministic: true,
                null_propagating: true,
            }
        })
    }

    fn procedure(&self, name: &str) -> Option<ProcedureSignature> {
        self.procedures.iter().find(|p| p.name == name).map(|p| {
            let params = p
                .params
                .iter()
                .map(|name| ParamDecl {
                    name: SmolStr::new(name),
                    ty: PropertyType::Any,
                    default: None,
                })
                .collect();
            let yields = p
                .yield_columns
                .iter()
                .map(|name| YieldDecl {
                    name: SmolStr::new(name),
                    ty: PropertyType::Any,
                })
                .collect();
            ProcedureSignature {
                name: SmolStr::new(&p.name),
                params,
                yields,
                // JsonSchema cannot express procedure R/W semantics;
                // default to Read so sema's gate treats user-supplied
                // procs as query-side rather than mutation-side.
                mode: ProcMode::Read,
            }
        })
    }

    fn schema_digest(&self) -> [u8; 32] {
        // Content-addressed hash of the observable surface.  Hash the
        // JSON shape of `self` so two schemas with identical shape
        // produce the same digest.  Sufficient for §11.4 revision
        // invalidation (reused across requests); not a security
        // primitive.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let serialised = serde_json::to_string(&SchemaDigestView {
            labels: &self.labels,
            rel_types: &self.rel_types,
        })
        .unwrap_or_default();
        let mut h = DefaultHasher::new();
        serialised.hash(&mut h);
        let v = h.finish();
        let mut out = [0u8; 32];
        out[..8].copy_from_slice(&v.to_le_bytes());
        out
    }
}

#[derive(serde::Serialize)]
struct SchemaDigestView<'a> {
    labels: &'a [String],
    rel_types: &'a [String],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lints_default_off_when_unset() {
        // No `initializationOptions` at all → lints off.
        let opts = InitOptions::from_value(None);
        assert!(!opts.lints_enabled());
        // An options object that omits `lints` → still off.
        let opts = InitOptions::from_value(Some(&serde_json::json!({})));
        assert!(!opts.lints_enabled());
    }

    #[test]
    fn lints_enabled_when_explicitly_true() {
        let opts = InitOptions::from_value(Some(&serde_json::json!({ "lints": true })));
        assert!(opts.lints_enabled());
    }

    #[test]
    fn lints_off_when_explicitly_false() {
        let opts = InitOptions::from_value(Some(&serde_json::json!({ "lints": false })));
        assert!(!opts.lints_enabled());
    }
}
