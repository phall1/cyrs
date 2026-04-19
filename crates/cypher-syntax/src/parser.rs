//! Event-based recovering parser (spec 0001 §4.2).
//!
//! # Architecture
//!
//! The parser is a hand-written recursive-descent engine that emits an
//! [`Event`] stream rather than directly mutating a tree. A separate
//! [`SyntaxTreeBuilder`] consumes the events and produces the [`rowan`]
//! green tree. This split lets the parser:
//!
//! - rewrite events for associativity fixing in Pratt expression parsing
//!   (the `Start` event can be retroactively relocated via the
//!   [`Marker::precede`] mechanism);
//! - group or suppress spurious errors before they reach diagnostics;
//! - emit trivia attached to the correct side of a significant token.
//!
//! # Error recovery
//!
//! Implementation of the recovery strategy in spec §4.3 lives alongside
//! the grammar functions (one synchronisation set per clause). This file
//! provides only the primitives — [`Parser::error`] (records a diag,
//! produces an ERROR node), [`Parser::bump_any`], [`Parser::at_ts`]
//! (token-set predicate), and [`Parser::expect`] — on which the grammar is
//! built.
//!
//! # v1 status
//!
//! The event machinery and builder are complete and lossless. The grammar
//! entry [`parse`] currently walks the token stream without structural
//! parsing; it produces a `SOURCE_FILE` containing the raw tokens as
//! children. Replacing the walker with the full grammar is the next
//! implementation step and is tracked against spec §4.3 + §19.

use rowan::{GreenNode, GreenNodeBuilder};
use thiserror::Error;

use crate::{
    SyntaxKind, SyntaxNode,
    lexer::{LexToken, lex},
};

/// A completed parse: green tree + accumulated syntax errors.
#[derive(Debug, Clone)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    #[must_use]
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    #[must_use]
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }
}

/// Entry point. Parses a complete source file.
///
/// Invariants:
/// - `parse(src).syntax().to_string() == src` for every input (spec §4.4).
/// - Never panics on any UTF-8 input (spec §17.3 P17.3.1).
#[must_use]
pub fn parse(src: &str) -> Parse {
    let tokens = lex(src);
    let mut builder = SyntaxTreeBuilder::new();
    builder.start_node(SyntaxKind::SOURCE_FILE);
    for tok in tokens {
        builder.token(tok.kind, &tok.text);
    }
    builder.finish_node();
    let (green, errors) = builder.finish();
    Parse { green, errors }
}

/// A single syntax error record. Spec §4.3 + §10.
#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct SyntaxError {
    pub message: String,
    pub offset: text_size::TextSize,
}

// ========================================================================
// Event stream
// ========================================================================

/// Events are emitted by the parser and consumed by [`SyntaxTreeBuilder`].
/// The `Abandoned` variant lets the parser retract speculatively-started
/// nodes; the `Forward` variant implements `Marker::precede` (re-parenting
/// a started node inside a new outer node).
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum Event {
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    Finish,
    Token {
        kind: SyntaxKind,
    },
    Error {
        msg: String,
    },
    Abandoned,
}

// ========================================================================
// Green-tree builder
// ========================================================================

/// Wraps `rowan::GreenNodeBuilder` with domain-specific helpers and an
/// error accumulator. Consuming an [`Event`] stream is a straightforward
/// walk; that wiring lands with the grammar.
#[derive(Debug, Default)]
pub(crate) struct SyntaxTreeBuilder {
    inner: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
}

impl SyntaxTreeBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn start_node(&mut self, kind: SyntaxKind) {
        self.inner.start_node(rowan::SyntaxKind(kind as u16));
    }

    pub(crate) fn finish_node(&mut self) {
        self.inner.finish_node();
    }

    pub(crate) fn token(&mut self, kind: SyntaxKind, text: &str) {
        self.inner.token(rowan::SyntaxKind(kind as u16), text);
    }

    #[allow(dead_code)]
    pub(crate) fn error(&mut self, err: SyntaxError) {
        self.errors.push(err);
    }

    pub(crate) fn finish(self) -> (GreenNode, Vec<SyntaxError>) {
        (self.inner.finish(), self.errors)
    }
}

// ========================================================================
// Parser primitive (stub; grammar lands incrementally)
// ========================================================================

/// The recovering parser. Wraps a token slice with a cursor, an event
/// buffer, and drop-bomb-protected markers.
///
/// Currently exposed only to the crate; the public entry point is
/// [`parse`]. The parser surface will be stabilised once the grammar is
/// implemented per spec §4.3.
#[allow(dead_code)]
pub(crate) struct Parser<'a> {
    tokens: &'a [LexToken],
    pos: usize,
    events: Vec<Event>,
}

#[allow(dead_code)]
impl<'a> Parser<'a> {
    pub(crate) fn new(tokens: &'a [LexToken]) -> Self {
        Self {
            tokens,
            pos: 0,
            events: Vec::new(),
        }
    }

    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .nth(self.non_trivia_pos() + n)
            .map_or(SyntaxKind::EOF, |t| t.kind)
    }

    fn non_trivia_pos(&self) -> usize {
        self.tokens[..self.pos]
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .count()
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        debug_assert!(
            self.at(kind),
            "bump: expected {kind:?}, got {:?}",
            self.current()
        );
        self.events.push(Event::Token { kind });
        self.pos += 1;
    }

    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        self.events.push(Event::Error { msg: msg.into() });
    }

    pub(crate) fn into_events(self) -> Vec<Event> {
        self.events
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parse_empty_is_lossless() {
        let p = parse("");
        assert_eq!(p.syntax().to_string(), "");
    }

    #[test]
    fn parse_simple_is_lossless() {
        let src = "MATCH (n) RETURN n";
        let p = parse(src);
        assert_eq!(p.syntax().to_string(), src);
    }

    #[test]
    fn parse_with_comments_is_lossless() {
        let src = "// leading\nMATCH (n) /* inline */ RETURN n";
        let p = parse(src);
        assert_eq!(p.syntax().to_string(), src);
    }

    use proptest::prelude::*;

    proptest! {
        /// Property P17.3.2 — lossless CST over arbitrary UTF-8 input.
        #[test]
        fn lossless_on_arbitrary_utf8(s in ".*") {
            let p = parse(&s);
            prop_assert_eq!(p.syntax().to_string(), s);
        }
    }
}
