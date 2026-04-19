//! `cypher-ast` — typed AST wrappers over the CST (spec 0001 §5).
//!
//! AST nodes are zero-cost wrappers around [`cypher_syntax::SyntaxNode`].
//! A node's methods navigate the underlying tree; no duplication, no
//! allocation. Per §5.2, the bulk of wrappers is generated from a
//! grammar description by a dev-only `xtask`; a small handwritten core
//! lives here until the generator is in place.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-ast/0.0.1")]

use cypher_syntax::{SyntaxKind, SyntaxNode};

/// Trait every typed AST wrapper implements. Modelled after the
/// rust-analyzer `AstNode` pattern.
pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(syntax: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);
        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }
            fn cast(syntax: SyntaxNode) -> Option<Self> {
                if Self::can_cast(syntax.kind()) {
                    Some(Self(syntax))
                } else {
                    None
                }
            }
            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(SourceFile, SOURCE_FILE);
ast_node!(Statement, STATEMENT);
ast_node!(MatchClause, MATCH_CLAUSE);
ast_node!(OptionalMatchClause, OPTIONAL_MATCH_CLAUSE);
ast_node!(WhereClause, WHERE_CLAUSE);
ast_node!(WithClause, WITH_CLAUSE);
ast_node!(ReturnClause, RETURN_CLAUSE);
ast_node!(CreateClause, CREATE_CLAUSE);
ast_node!(MergeClause, MERGE_CLAUSE);
ast_node!(SetClause, SET_CLAUSE);
ast_node!(RemoveClause, REMOVE_CLAUSE);
ast_node!(DeleteClause, DELETE_CLAUSE);
ast_node!(UnwindClause, UNWIND_CLAUSE);
ast_node!(CallClause, CALL_CLAUSE);

ast_node!(Pattern, PATTERN);
ast_node!(NodePattern, NODE_PATTERN);
ast_node!(RelPattern, REL_PATTERN);

impl SourceFile {
    /// Iterate over the statements in this source file.
    pub fn statements(&self) -> impl Iterator<Item = Statement> {
        self.syntax().children().filter_map(Statement::cast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cypher_syntax::parse;

    #[test]
    fn cast_source_file() {
        let parse = parse("");
        let src = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
        assert_eq!(src.statements().count(), 0);
    }
}
