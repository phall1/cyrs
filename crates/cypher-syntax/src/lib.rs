//! `cypher-syntax` — lexer, recovering parser, lossless CST.
//!
//! Reference: spec 0001 §4. The [`SyntaxKind`] enum in [`kind`] is the
//! canonical grammar reference; treat it as authoritative. The CST is a
//! [`rowan`] green/red tree parameterised by [`Lang`]; it preserves every
//! byte of the input (including trivia and malformed fragments).
//!
//! The parser is event-based (spec 0001 §4.2): it emits an event stream
//! that a builder later uses to construct the rowan tree. This lets the
//! parser rewrite events for associativity and error grouping before tree
//! construction commits.
//!
//! Everything in this crate is domain-free (spec §2). There are no
//! references to any consumer-specific concept.

// Embedders: see ../../docs/integration-depth.md before depending on this surface.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-syntax/0.0.1")]

pub(crate) mod builder;
pub mod edit;
pub(crate) mod grammar;
pub mod kind;
pub mod lexer;
pub mod line_index;
pub mod parser;

pub use edit::{TextEdit, incremental_reparse};
pub use kind::SyntaxKind;
pub use lexer::{LexError, LexToken, lex, validate_tokens};
pub use line_index::{LineCol, LineIndex, WideLineCol};
pub use parser::{Parse, SyntaxError, parse};

use rowan::Language;

/// Language marker for the Cypher rowan tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}

impl Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_u16(raw.0).expect("SyntaxKind round-trip: invalid raw kind")
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

/// Cypher-flavoured alias for `rowan::SyntaxNode` — a typed handle on
/// a node in the CST.
pub type SyntaxNode = rowan::SyntaxNode<Lang>;
/// Cypher-flavoured alias for `rowan::SyntaxToken` — a leaf token in
/// the CST.
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
/// Cypher-flavoured alias for `rowan::SyntaxElement` — a sum of
/// `SyntaxNode | SyntaxToken` used when walking a tree.
pub type SyntaxElement = rowan::SyntaxElement<Lang>;
/// Cypher-flavoured alias for `rowan::SyntaxNodeChildren` — a node's
/// immediate child iterator.
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<Lang>;
/// Cypher-flavoured alias for `rowan::api::PreorderWithTokens` — a
/// pre-order walk that visits both nodes and tokens.
pub type PreorderWithTokens = rowan::api::PreorderWithTokens<Lang>;

/// Byte offset range in the source text. Re-exported from `text-size` for
/// convenience so downstream crates can depend only on `cypher-syntax`.
pub use text_size::{TextLen, TextRange, TextSize};
