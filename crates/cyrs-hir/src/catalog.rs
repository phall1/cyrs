//! HIR for GQL catalog-DDL statements (ISO/IEC 39075:2024 §14.14;
//! spec amendment 2026-05-19 cy-5e3f; bead cy-v5u6).
//!
//! # Architectural note (cy-v5u6)
//!
//! The query HIR ([`crate::Statement`]) is shaped around the `STATEMENT`
//! CST production: a vector of [`crate::Clause`]s with name resolution
//! over [`crate::VarId`]s. Catalog DDL statements (`CREATE GRAPH`,
//! `CREATE SCHEMA`) sit at top-level statement scope *alongside* the
//! query `STATEMENT` node (see `cyrs_syntax::grammar::source_file`);
//! they carry no clauses, no scope graph, and no variable bindings.
//!
//! Rather than expand [`crate::Statement`] (which would force every
//! consumer to widen its match-arms and would deepen the embedder
//! coupling), we introduce a sibling [`CatalogHir`] enum and a separate
//! lowering entry-point ([`lower_catalog_from_parse`]) that walks the
//! `SOURCE_FILE` for catalog children. The two HIRs are independent and
//! can be consumed together by an embedder that wants both query +
//! catalog views of the same source file.
//!
//! The Plan IR mapping is intentionally deferred (cy-plan-catalog-session
//! is a separate bead) — embedders that want to act on a parsed catalog
//! op read [`CatalogHir`] directly.

use cyrs_syntax::{Parse, SyntaxKind, SyntaxNode, TextRange};
use smol_str::SmolStr;

use crate::HirId;

/// A single lowered catalog-DDL statement.
///
/// Each variant carries its own [`HirId`] (allocated against a fresh
/// per-statement counter — see [`CatalogStatement::alloc_id`]) and the
/// originating CST span for diagnostics.
//
// NOTE (cy-v5u6): non-exhaustive so future catalog statements
// (CREATE GRAPH TYPE, DROP GRAPH, etc.) can be added without breaking
// downstream consumers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CatalogHir {
    /// `CREATE GRAPH <name> [<type-ref>] [<source>]`.
    CreateGraph {
        id: HirId,
        /// Bound graph name. Concatenated path / ident text — `/foo/bar`
        /// for a path form, or the bare identifier text otherwise.
        name: SmolStr,
        /// `true` iff the name was spelled as a path (`/foo/bar`).
        name_is_path: bool,
        /// Optional graph-type reference (`ANY`, `::name`, `TYPED name`,
        /// `name`, inline `{…}`, or `::{…}`).
        graph_type: Option<GraphTypeHir>,
        /// Optional source qualifier (`LIKE …` or `AS COPY OF …`).
        source: Option<GraphSourceHir>,
        /// Source span covering the entire `CREATE GRAPH …` statement.
        span: TextRange,
    },
    /// `CREATE SCHEMA /seg[/seg…]`.
    CreateSchema {
        id: HirId,
        /// Path segments in source order. `["foo", "bar"]` for
        /// `/foo/bar`. Recovery shapes (bare identifier instead of a
        /// path) collapse to a single-element vector.
        segments: Vec<SmolStr>,
        /// Source span covering the entire `CREATE SCHEMA …` statement.
        span: TextRange,
    },
}

impl CatalogHir {
    /// Return the [`HirId`] identifying this catalog op.
    pub fn id(&self) -> HirId {
        match self {
            CatalogHir::CreateGraph { id, .. } | CatalogHir::CreateSchema { id, .. } => *id,
        }
    }

    /// Source span of the catalog op.
    pub fn span(&self) -> TextRange {
        match self {
            CatalogHir::CreateGraph { span, .. } | CatalogHir::CreateSchema { span, .. } => *span,
        }
    }
}

/// HIR shape for the optional graph-type reference following a
/// `CREATE GRAPH <name>`.
//
// NOTE (cy-v5u6): keeps the inline-literal cases as opaque markers
// (`Inline` / `InlineDoubleColon`) — the inner element shape is
// captured by the CST and re-read by consumers that care; v1 catalog
// HIR does not duplicate the element list because no v1 consumer
// needs it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum GraphTypeHir {
    /// `ANY` — open graph type.
    Any,
    /// `::name` named ref.
    DoubleColonNamed(SmolStr),
    /// `::{…}` inline literal under a double-colon prefix.
    DoubleColonInline,
    /// `TYPED name` named ref.
    TypedNamed(SmolStr),
    /// Bare `{…}` inline literal.
    Inline,
    /// Bare `name` ref (the ambiguous fifth surface form).
    BareNamed(SmolStr),
}

/// HIR shape for the optional source qualifier.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum GraphSourceHir {
    /// `LIKE <name>` — structural clone reference.
    Like { name: SmolStr, name_is_path: bool },
    /// `AS COPY OF <name>` — clone with data.
    AsCopyOf { name: SmolStr, name_is_path: bool },
}

/// Per-statement HIR-id counter for catalog ops.
///
/// Mirrors [`crate::Statement::alloc_id`] but lives outside the query
/// HIR so catalog statements do not perturb the query-side id space.
/// `next_id` starts at 1 so [`HirId::DUMMY`] remains distinguishable.
#[derive(Debug, Clone)]
pub struct CatalogStatement {
    next_id: u32,
}

impl Default for CatalogStatement {
    fn default() -> Self {
        Self { next_id: 1 }
    }
}

impl CatalogStatement {
    /// Allocate a fresh [`HirId`] for a catalog node.
    pub fn alloc_id(&mut self) -> HirId {
        let id = HirId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("HirId counter overflowed u32 within a catalog statement");
        id
    }
}

/// Walk every direct `CATALOG_CREATE_GRAPH_STMT` /
/// `CATALOG_CREATE_SCHEMA_STMT` child of `SOURCE_FILE` and lower each
/// into a [`CatalogHir`]. Returns an empty vector when the source file
/// has no catalog ops.
///
/// Multiple ops can appear in a single source file separated by `;` or
/// the GQL `NEXT` token (ISO/IEC 39075:2024 §14.14); this lowerer
/// returns them in source order.
pub fn lower_catalog_from_parse(parse: &Parse) -> Vec<CatalogHir> {
    let root = parse.syntax();
    let mut ctx = CatalogStatement::default();
    let mut out = Vec::new();
    for child in root.children() {
        match child.kind() {
            SyntaxKind::CATALOG_CREATE_GRAPH_STMT => {
                if let Some(op) = lower_create_graph(&mut ctx, child) {
                    out.push(op);
                }
            }
            SyntaxKind::CATALOG_CREATE_SCHEMA_STMT => {
                if let Some(op) = lower_create_schema(&mut ctx, child) {
                    out.push(op);
                }
            }
            _ => {}
        }
    }
    out
}

fn lower_create_graph(ctx: &mut CatalogStatement, node: SyntaxNode) -> Option<CatalogHir> {
    let stmt = cyrs_ast::CatalogCreateGraphStmt::cast(node.clone())?;
    let id = ctx.alloc_id();
    let span = node.text_range();
    let (name, name_is_path) = match stmt.graph_name() {
        Some(n) => {
            let is_path = n.path().is_some();
            (SmolStr::new(n.text()), is_path)
        }
        None => (SmolStr::default(), false),
    };
    let graph_type = stmt.graph_type_ref().and_then(lower_graph_type_ref);
    let source = stmt.graph_source().and_then(lower_graph_source);
    Some(CatalogHir::CreateGraph {
        id,
        name,
        name_is_path,
        graph_type,
        source,
        span,
    })
}

fn lower_create_schema(ctx: &mut CatalogStatement, node: SyntaxNode) -> Option<CatalogHir> {
    let stmt = cyrs_ast::CatalogCreateSchemaStmt::cast(node.clone())?;
    let id = ctx.alloc_id();
    let span = node.text_range();
    let segments: Vec<SmolStr> = if let Some(path) = stmt.path_ident() {
        path.segments().into_iter().map(SmolStr::new).collect()
    } else if let Some(name) = stmt.graph_name_fallback() {
        // Recovery: bare IDENT instead of path form. Capture the single
        // segment so well-formedness sema can still flag it.
        let txt = name.text();
        if txt.is_empty() {
            Vec::new()
        } else {
            vec![SmolStr::new(txt)]
        }
    } else {
        Vec::new()
    };
    Some(CatalogHir::CreateSchema { id, segments, span })
}

fn lower_graph_type_ref(ty: cyrs_ast::GraphTypeRef) -> Option<GraphTypeHir> {
    match ty.kind()? {
        cyrs_ast::GraphTypeRefKind::Any => Some(GraphTypeHir::Any),
        cyrs_ast::GraphTypeRefKind::DoubleColonNamed(name) => {
            Some(GraphTypeHir::DoubleColonNamed(SmolStr::new(name)))
        }
        cyrs_ast::GraphTypeRefKind::DoubleColonInline => Some(GraphTypeHir::DoubleColonInline),
        cyrs_ast::GraphTypeRefKind::TypedNamed(name) => {
            Some(GraphTypeHir::TypedNamed(SmolStr::new(name)))
        }
        cyrs_ast::GraphTypeRefKind::Inline => Some(GraphTypeHir::Inline),
        cyrs_ast::GraphTypeRefKind::BareNamed(name) => {
            Some(GraphTypeHir::BareNamed(SmolStr::new(name)))
        }
    }
}

fn lower_graph_source(src: cyrs_ast::GraphSource) -> Option<GraphSourceHir> {
    let kind = src.kind()?;
    let to_pair = |n: cyrs_ast::GraphName| (SmolStr::new(n.text()), n.path().is_some());
    match kind {
        cyrs_ast::GraphSourceKind::Like(n) => {
            let (name, name_is_path) = to_pair(n);
            Some(GraphSourceHir::Like { name, name_is_path })
        }
        cyrs_ast::GraphSourceKind::AsCopyOf(n) => {
            let (name, name_is_path) = to_pair(n);
            Some(GraphSourceHir::AsCopyOf { name, name_is_path })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrs_syntax::parse;

    fn lower(src: &str) -> Vec<CatalogHir> {
        let p = parse(src);
        lower_catalog_from_parse(&p)
    }

    #[test]
    fn lowers_create_graph_any() {
        let ops = lower("CREATE GRAPH mygraph ANY");
        assert_eq!(ops.len(), 1);
        let CatalogHir::CreateGraph {
            name,
            name_is_path,
            graph_type,
            source,
            ..
        } = &ops[0]
        else {
            panic!("expected CreateGraph");
        };
        assert_eq!(name, "mygraph");
        assert!(!name_is_path);
        assert!(matches!(graph_type, Some(GraphTypeHir::Any)));
        assert!(source.is_none());
    }

    #[test]
    fn lowers_create_graph_double_colon_named() {
        let ops = lower("CREATE GRAPH mygraph ::mytype");
        let CatalogHir::CreateGraph { graph_type, .. } = &ops[0] else {
            panic!("expected CreateGraph");
        };
        let Some(GraphTypeHir::DoubleColonNamed(name)) = graph_type else {
            panic!("expected DoubleColonNamed");
        };
        assert_eq!(name, "mytype");
    }

    #[test]
    fn lowers_create_graph_typed_named() {
        let ops = lower("CREATE GRAPH mygraph TYPED mytype");
        let CatalogHir::CreateGraph { graph_type, .. } = &ops[0] else {
            panic!("expected CreateGraph");
        };
        let Some(GraphTypeHir::TypedNamed(name)) = graph_type else {
            panic!("expected TypedNamed");
        };
        assert_eq!(name, "mytype");
    }

    #[test]
    fn lowers_create_graph_inline_double_colon() {
        let ops = lower("CREATE GRAPH mygraph ::{(Person :Person {name STRING})}");
        let CatalogHir::CreateGraph { graph_type, .. } = &ops[0] else {
            panic!("expected CreateGraph");
        };
        assert!(matches!(graph_type, Some(GraphTypeHir::DoubleColonInline)));
    }

    #[test]
    fn lowers_create_graph_inline_literal() {
        let ops = lower("CREATE GRAPH mygraph {(Person :Person {name STRING})}");
        let CatalogHir::CreateGraph { graph_type, .. } = &ops[0] else {
            panic!("expected CreateGraph");
        };
        assert!(matches!(graph_type, Some(GraphTypeHir::Inline)));
    }

    #[test]
    fn lowers_create_graph_bare_named() {
        let ops = lower("CREATE GRAPH mygraph mygraphtype");
        let CatalogHir::CreateGraph { graph_type, .. } = &ops[0] else {
            panic!("expected CreateGraph");
        };
        let Some(GraphTypeHir::BareNamed(name)) = graph_type else {
            panic!("expected BareNamed");
        };
        assert_eq!(name, "mygraphtype");
    }

    #[test]
    fn lowers_create_graph_path_like_source() {
        let ops = lower("CREATE GRAPH /mygraph LIKE /mysrc");
        let CatalogHir::CreateGraph {
            name,
            name_is_path,
            source,
            ..
        } = &ops[0]
        else {
            panic!("expected CreateGraph");
        };
        assert_eq!(name, "/mygraph");
        assert!(*name_is_path);
        let Some(GraphSourceHir::Like {
            name: src,
            name_is_path: src_is_path,
        }) = source
        else {
            panic!("expected Like");
        };
        assert_eq!(src, "/mysrc");
        assert!(*src_is_path);
    }

    #[test]
    fn lowers_create_graph_as_copy_of() {
        let ops = lower("CREATE GRAPH mygraph ANY AS COPY OF mysrc");
        let CatalogHir::CreateGraph { source, .. } = &ops[0] else {
            panic!("expected CreateGraph");
        };
        let Some(GraphSourceHir::AsCopyOf {
            name: src,
            name_is_path,
        }) = source
        else {
            panic!("expected AsCopyOf");
        };
        assert_eq!(src, "mysrc");
        assert!(!name_is_path);
    }

    #[test]
    fn lowers_create_schema_single_segment() {
        let ops = lower("CREATE SCHEMA /myschema");
        let CatalogHir::CreateSchema { segments, .. } = &ops[0] else {
            panic!("expected CreateSchema");
        };
        assert_eq!(segments.as_slice(), &[SmolStr::new("myschema")]);
    }

    #[test]
    fn lowers_create_schema_multi_segment() {
        let ops = lower("CREATE SCHEMA /foo/myschema");
        let CatalogHir::CreateSchema { segments, .. } = &ops[0] else {
            panic!("expected CreateSchema");
        };
        assert_eq!(
            segments.as_slice(),
            &[SmolStr::new("foo"), SmolStr::new("myschema")],
        );
    }

    #[test]
    fn lowers_multiple_ops_with_next_separator() {
        let ops = lower("CREATE SCHEMA /foo\nNEXT CREATE SCHEMA /bar");
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], CatalogHir::CreateSchema { .. }));
        assert!(matches!(ops[1], CatalogHir::CreateSchema { .. }));
        // HirIds are monotonic within the catalog statement scope.
        assert_eq!(ops[0].id(), HirId(1));
        assert_eq!(ops[1].id(), HirId(2));
    }

    #[test]
    fn empty_when_no_catalog_op() {
        let ops = lower("MATCH (n) RETURN n");
        assert!(ops.is_empty());
    }
}
