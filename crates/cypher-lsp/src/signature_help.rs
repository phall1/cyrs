//! v1 `textDocument/signatureHelp` engine (spec §14.2, bead cy-f2e).
//!
//! Heuristic, no CST walk: at the cursor, scan backwards for the
//! innermost unclosed `(`.  If the token immediately before that `(`
//! is an identifier and the workspace-scoped schema (if any) knows a
//! function with that name, return a `SignatureHelp` describing its
//! parameter list, with `active_parameter` set to the count of
//! top-level commas between the `(` and the cursor.
//!
//! Unknown functions (or no schema loaded) still return a well-formed
//! response with an empty `signatures` list — the LSP spec lets
//! clients treat that as "no info for this call site" without popping
//! an error.
//!
//! # Why no CST walk?
//!
//! The CST is recovery-oriented and the parser only guarantees a tree
//! for the successful prefix.  Byte-scanning handles half-typed calls
//! like `count(` where the AST might not have a `CallExpr` node yet,
//! which is exactly the shape signatureHelp is supposed to light up.

use cypher_db::{Database, FileId};
use cypher_schema::{FunctionSignature, ParamDecl, PropertyType};
use cypher_syntax::{LineIndex, TextSize};
use lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

/// Compute signature-help content for the given cursor.
///
/// Returns `None` when the cursor is not inside a call.  An unknown
/// function name returns `Some(SignatureHelp { signatures: vec![], .. })`
/// so the client registers "inside a call, no signature available"
/// instead of "still outside a call, keep listening for trigger chars".
pub(crate) fn compute(db: &Database, file_id: FileId, position: Position) -> Option<SignatureHelp> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let (open_paren_at, active_parameter) = find_call_context(&source, offset)?;
    let name = identifier_before(&source, open_paren_at)?;

    let signatures = match db.schema().and_then(|s| s.function(&name)) {
        Some(sig) => vec![render_signature(&sig)],
        None => Vec::new(),
    };

    Some(SignatureHelp {
        signatures,
        active_signature: Some(0),
        active_parameter: Some(active_parameter),
    })
}

/// Locate the innermost unclosed `(` before `offset`, returning its
/// byte position plus the top-level comma count between it and the
/// cursor (the `active_parameter` index).
///
/// Returns `None` when the cursor is outside any paren group — i.e.
/// paren depth is already 0 at `offset`.
fn find_call_context(source: &str, offset: TextSize) -> Option<(TextSize, u32)> {
    let upto: usize = u32::from(offset) as usize;
    let bytes = source.as_bytes().get(..upto)?;

    let mut commas_at_target_depth: u32 = 0;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut string_delim: u8 = 0;

    // Walk backwards; remember the innermost `(` (first `(` we see at
    // depth that goes negative).  Strings are tracked naively — the
    // lexer's richer escape handling is overkill for a cursor
    // heuristic.
    for (i, &b) in bytes.iter().enumerate().rev() {
        if in_string {
            if b == string_delim {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' | b'\'' | b'`' => {
                in_string = true;
                string_delim = b;
            }
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                depth -= 1;
                if depth < 0 {
                    // If the innermost opener is `(` we're in a call
                    // context; brackets/braces escape upward.
                    if b == b'(' {
                        return Some((TextSize::try_from(i).ok()?, commas_at_target_depth));
                    }
                    return None;
                }
            }
            b',' if depth == 0 => commas_at_target_depth += 1,
            _ => {}
        }
    }
    None
}

/// Identifier immediately preceding `offset`, skipping whitespace.
///
/// ASCII-only: Cypher identifiers are `[A-Za-z_][A-Za-z0-9_]*` plus
/// backtick-quoted forms (which we do not currently surface here —
/// the schema's function table keys off the unquoted name anyway).
fn identifier_before(source: &str, offset: TextSize) -> Option<String> {
    let upto: usize = u32::from(offset) as usize;
    let bytes = source.as_bytes().get(..upto)?;
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|p| p + 1)?;
    let start = bytes[..end]
        .iter()
        .rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.'))
        .map_or(0, |p| p + 1);
    if start >= end {
        return None;
    }
    let ident = std::str::from_utf8(&bytes[start..end]).ok()?;
    let first = ident.as_bytes().first()?;
    if !(first.is_ascii_alphabetic() || *first == b'_') {
        return None;
    }
    Some(ident.to_owned())
}

/// Render a schema `FunctionSignature` into the LSP shape.  Labels
/// are computed so `active_parameter` indexes into them directly; the
/// `ParameterLabel::Simple` form lets editors highlight the current
/// parameter in the signature popup.
fn render_signature(sig: &FunctionSignature) -> SignatureInformation {
    let mut parameters: Vec<ParameterInformation> = sig.params.iter().map(param_info).collect();
    if let Some(v) = &sig.variadic {
        parameters.push(ParameterInformation {
            label: ParameterLabel::Simple(format!("...{}: {}", v.name, property_type_name(&v.ty))),
            documentation: None,
        });
    }

    let param_labels: Vec<String> = parameters
        .iter()
        .map(|p| match &p.label {
            ParameterLabel::Simple(s) => s.clone(),
            ParameterLabel::LabelOffsets(_) => String::new(),
        })
        .collect();
    let label = format!("{}({})", sig.name, param_labels.join(", "));

    SignatureInformation {
        label,
        documentation: None,
        parameters: Some(parameters),
        active_parameter: None,
    }
}

fn param_info(p: &ParamDecl) -> ParameterInformation {
    ParameterInformation {
        label: ParameterLabel::Simple(format!("{}: {}", p.name, property_type_name(&p.ty))),
        documentation: None,
    }
}

/// Stable, human-readable stringification of a `PropertyType`.  We
/// avoid `Debug` because it would surface internals like
/// `List(Box<PropertyType>)` which reads badly in a tooltip.
fn property_type_name(ty: &PropertyType) -> String {
    match ty {
        PropertyType::String => "String".to_owned(),
        PropertyType::Int => "Integer".to_owned(),
        PropertyType::Float => "Float".to_owned(),
        PropertyType::Bool => "Boolean".to_owned(),
        PropertyType::Date => "Date".to_owned(),
        PropertyType::Datetime => "Datetime".to_owned(),
        PropertyType::List(inner) => format!("List<{}>", property_type_name(inner)),
        PropertyType::Enum(name, _) | PropertyType::Opaque(name) => name.to_string(),
        PropertyType::Any => "Any".to_owned(),
    }
}

fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cypher_db::{Database, DialectMode};
    use cypher_schema::{
        EndpointDecl, FnCategories, FunctionSignature, ProcedureSignature, PropertyDecl, ReturnTy,
        SchemaProvider,
    };
    use smol_str::SmolStr;
    use std::path::Path;
    use std::sync::Arc;

    /// Inline fixture schema exposing a single `toInteger(x: String) -> Int`
    /// function so the "known function" signatureHelp path can be
    /// exercised without depending on cypher-testkit (dev-dep cycle) or
    /// `cypher-schema::StandardLibrary` (not part of the v1 surface yet).
    struct FixtureSchema;

    impl SchemaProvider for FixtureSchema {
        fn labels(&self) -> Vec<SmolStr> {
            vec![]
        }
        fn relationship_types(&self) -> Vec<SmolStr> {
            vec![]
        }
        fn node_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
            None
        }
        fn relationship_properties(&self, _: &str) -> Option<Vec<PropertyDecl>> {
            None
        }
        fn relationship_endpoints(&self, _: &str) -> Vec<EndpointDecl> {
            vec![]
        }
        fn inverse_of(&self, _: &str) -> Option<SmolStr> {
            None
        }
        fn function(&self, name: &str) -> Option<FunctionSignature> {
            if name == "toInteger" {
                Some(FunctionSignature {
                    name: SmolStr::new("toInteger"),
                    params: vec![cypher_schema::ParamDecl {
                        name: SmolStr::new("value"),
                        ty: cypher_schema::PropertyType::String,
                        default: None,
                    }],
                    variadic: None,
                    return_ty: ReturnTy::Constant(cypher_schema::PropertyType::Int),
                    categories: FnCategories {
                        pure: true,
                        aggregate: false,
                        deterministic: true,
                    },
                })
            } else {
                None
            }
        }
        fn procedure(&self, _: &str) -> Option<ProcedureSignature> {
            None
        }
        fn schema_digest(&self) -> [u8; 32] {
            [0u8; 32]
        }
    }

    fn db_with_source(source: &str) -> (Database, cypher_db::FileId) {
        let mut db = Database::new();
        let id = db.open_file(Path::new("t.cyp"), source.into(), DialectMode::GqlAligned);
        db.set_schema(Some(Arc::new(FixtureSchema) as Arc<dyn SchemaProvider>));
        (db, id)
    }

    #[test]
    fn known_function_returns_signature() {
        // Cursor right after the opening paren of `toInteger(`.
        let (db, id) = db_with_source("RETURN toInteger(");
        let sig = compute(
            &db,
            id,
            Position {
                line: 0,
                character: 17,
            },
        )
        .expect("inside call → Some");
        assert_eq!(sig.signatures.len(), 1, "one signature expected");
        let info = &sig.signatures[0];
        assert!(
            info.label.starts_with("toInteger("),
            "label must name the function: {:?}",
            info.label
        );
        assert!(
            info.label.contains("value: String"),
            "label must carry param name + type: {:?}",
            info.label
        );
        assert_eq!(sig.active_parameter, Some(0));
    }

    #[test]
    fn unknown_function_returns_empty_signatures() {
        let (db, id) = db_with_source("RETURN nope(");
        let sig = compute(
            &db,
            id,
            Position {
                line: 0,
                character: 12,
            },
        )
        .expect("inside call → Some");
        assert!(
            sig.signatures.is_empty(),
            "unknown fn must return empty signatures; got {:?}",
            sig.signatures
        );
        assert_eq!(sig.active_parameter, Some(0));
    }

    #[test]
    fn outside_parens_returns_none() {
        let (db, id) = db_with_source("MATCH (n) RETURN n");
        let sig = compute(
            &db,
            id,
            Position {
                line: 0,
                character: 0,
            },
        );
        assert!(sig.is_none(), "outside any call → None");
    }
}
