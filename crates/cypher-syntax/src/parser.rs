//! Event-based recovering parser (spec 0001 §4.2, §4.3, §4.4, §4.6).
//!
//! # Architecture
//!
//! The parser is a hand-written recursive-descent engine that emits an
//! [`Event`] stream rather than directly mutating a tree. A separate
//! [`SyntaxTreeBuilder`](crate::builder::SyntaxTreeBuilder) — living in
//! [`crate::builder`] — consumes the events and produces the [`rowan`]
//! green tree. This split lets the parser:
//!
//! - rewrite events for associativity fixing in Pratt expression parsing
//!   (the `Start` event can be retroactively relocated via the
//!   [`Marker::precede`] mechanism);
//! - group or suppress spurious errors before they reach diagnostics;
//! - interleave trivia tokens (whitespace, comments) between significant
//!   tokens so the constructed tree is byte-lossless per §4.4.
//!
//! # Error recovery
//!
//! Per spec §4.3 the recovering parser exposes two primitives the grammar
//! builds on:
//!
//! - **Synchronisation-set skip** ([`Parser::recover_until`]): when a
//!   clause fails, tokens are bumped into an `ERROR` node until we see a
//!   clause-level keyword, a `;`, or EOF.
//! - **Virtual-token insertion** ([`Parser::expect`]): when a required
//!   closer/keyword is missing, an `ERROR` node of zero width is recorded
//!   at the expected position; the parser continues.
//!
//! Both primitives are shared across productions; per-production recovery
//! tables (spec §4.3) land in a later bead (`cy-2vh`).
//!
//! # Losslessness invariant
//!
//! Spec §4.4: `parse(s).syntax().to_string() == s` for every input. Trivia
//! and `ERROR` tokens are preserved in the tree; the property test
//! `lossless_on_arbitrary_utf8` enforces the invariant.

use drop_bomb::DropBomb;
use rowan::GreenNode;
use thiserror::Error;

use crate::{
    SyntaxKind, SyntaxNode,
    builder::SyntaxTreeBuilder,
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
    let mut parser = Parser::new(&tokens);
    crate::grammar::source_file(&mut parser);
    let events = parser.into_events();
    let (green, errors) = SyntaxTreeBuilder::build(&tokens, events);
    Parse { green, errors }
}

/// A single syntax error record. Spec §4.3 + §10.
///
/// `cypher-syntax` does not depend on `cypher-diag` (§3.1), so this is a
/// local stand-in carrying just the minimum a later pass needs to lift it
/// into a full `Diagnostic`: a human-readable message and the byte offset
/// at which it was emitted.
#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct SyntaxError {
    pub message: String,
    pub offset: text_size::TextSize,
}

// ========================================================================
// Event stream
// ========================================================================

/// Events are emitted by the parser and consumed by
/// [`SyntaxTreeBuilder`](crate::builder::SyntaxTreeBuilder).
///
/// `Start` carries an optional forward-parent index implementing
/// [`Marker::precede`] (re-parenting a started node inside a new outer
/// node); [`Event::Abandoned`] retracts a speculatively-started node.
///
/// The event stream is flat: Start/Finish pairs define nesting, Token
/// events are emitted in source order alongside trivia that the builder
/// re-interleaves from the underlying token slice.
#[derive(Debug)]
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
        offset: text_size::TextSize,
    },
    Abandoned,
}

impl Event {
    /// Placeholder used by the builder when it swap-removes an event
    /// during forward-parent chain resolution.
    pub(crate) fn tombstone() -> Self {
        Event::Abandoned
    }
}

// ========================================================================
// Parser engine
// ========================================================================

/// Token-set bitmap used by `at_ts`/`recover_until`. Backed by an
/// 8×u128 array so we can represent every [`SyntaxKind`] value in
/// `0..1024` without collisions (the enum's high-water mark is
/// [`SyntaxKind::EOF`] = 769). Every construction is `const`-friendly so
/// grammar code can build sets at compile time.
#[derive(Copy, Clone, Debug)]
pub(crate) struct TokenSet([u128; TOKEN_SET_WORDS]);

const TOKEN_SET_WORDS: usize = 8;
#[allow(clippy::cast_possible_truncation)]
const TOKEN_SET_CAPACITY: u16 = (TOKEN_SET_WORDS as u16) * 128;

impl TokenSet {
    pub(crate) const EMPTY: TokenSet = TokenSet([0; TOKEN_SET_WORDS]);

    pub(crate) const fn new(kinds: &[SyntaxKind]) -> Self {
        let mut mask = [0u128; TOKEN_SET_WORDS];
        let mut i = 0;
        while i < kinds.len() {
            let raw = kinds[i] as u16;
            assert!(raw < TOKEN_SET_CAPACITY, "TokenSet: kind out of range");
            let word = (raw / 128) as usize;
            let bit = raw % 128;
            mask[word] |= 1u128 << bit;
            i += 1;
        }
        TokenSet(mask)
    }

    pub(crate) const fn union(self, other: TokenSet) -> TokenSet {
        let mut out = [0u128; TOKEN_SET_WORDS];
        let mut i = 0;
        while i < TOKEN_SET_WORDS {
            out[i] = self.0[i] | other.0[i];
            i += 1;
        }
        TokenSet(out)
    }

    pub(crate) const fn contains(self, kind: SyntaxKind) -> bool {
        let raw = kind as u16;
        if raw >= TOKEN_SET_CAPACITY {
            return false;
        }
        let word = (raw / 128) as usize;
        let bit = raw % 128;
        (self.0[word] & (1u128 << bit)) != 0
    }
}

impl Default for TokenSet {
    fn default() -> Self {
        TokenSet::EMPTY
    }
}

/// The recovering parser. Owns the token slice plus an in-flight event
/// buffer. A non-trivia cursor jumps over whitespace/comment tokens so
/// grammar code never sees them — the builder reassembles trivia when
/// consuming events.
pub(crate) struct Parser<'a> {
    tokens: &'a [LexToken],
    /// Position in `tokens` — the index of the next *significant* token.
    /// Trivia between significant tokens is implicitly skipped.
    pos: usize,
    events: Vec<Event>,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(tokens: &'a [LexToken]) -> Self {
        Self {
            tokens,
            pos: skip_trivia(tokens, 0),
            events: Vec::new(),
        }
    }

    /// Current significant token kind, or `EOF` if exhausted.
    pub(crate) fn current(&self) -> SyntaxKind {
        self.tokens
            .get(self.pos)
            .map_or(SyntaxKind::EOF, |t| t.kind)
    }

    /// Look ahead `n` significant tokens. `nth(0) == current()`.
    #[allow(dead_code)]
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        let mut idx = self.pos;
        let mut remaining = n;
        loop {
            if idx >= self.tokens.len() {
                return SyntaxKind::EOF;
            }
            if self.tokens[idx].kind.is_trivia() {
                idx += 1;
                continue;
            }
            if remaining == 0 {
                return self.tokens[idx].kind;
            }
            remaining -= 1;
            idx += 1;
        }
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn at_ts(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    /// Case-insensitive match against a contextual keyword spelled as an
    /// identifier. Cypher allows unreserved identifiers (e.g. `WITH` when
    /// scoped by context); for v1 we only need this for the composite
    /// operators `STARTS WITH` / `ENDS WITH`, which are handled via the
    /// already-tokenised `STARTS_KW` / `ENDS_KW` and thus do not need the
    /// hook. Left here as a single call site in case later beads do.
    #[allow(dead_code)]
    pub(crate) fn at_contextual(&self, ident: &str) -> bool {
        self.current() == SyntaxKind::IDENT
            && self
                .tokens
                .get(self.pos)
                .is_some_and(|t| t.text.eq_ignore_ascii_case(ident))
    }

    /// Byte offset of the current token (or end-of-input if exhausted).
    fn current_offset(&self) -> text_size::TextSize {
        self.tokens.get(self.pos).map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(text_size::TextSize::from(0), |t| t.range.end())
            },
            |t| t.range.start(),
        )
    }

    /// Consume whatever token is current (including EOF-no-op) and emit it
    /// as a Token event. Use sparingly — prefer `bump(kind)` when the kind
    /// is known for the debug assertion.
    pub(crate) fn bump_any(&mut self) {
        let kind = self.current();
        if kind == SyntaxKind::EOF {
            return;
        }
        self.events.push(Event::Token { kind });
        self.pos += 1;
        self.pos = skip_trivia(self.tokens, self.pos);
    }

    /// Consume the current token, asserting it has the given kind in debug.
    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        debug_assert!(
            self.at(kind),
            "bump: expected {kind:?}, got {:?}",
            self.current()
        );
        self.bump_any();
    }

    /// Consume `kind` if present; return whether it was.
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump(kind);
            true
        } else {
            false
        }
    }

    /// Record a diagnostic at the current position without consuming.
    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        let offset = self.current_offset();
        self.events.push(Event::Error {
            msg: msg.into(),
            offset,
        });
    }

    /// Consume one token and wrap it in an ERROR node alongside a message.
    #[allow(dead_code)]
    pub(crate) fn err_and_bump(&mut self, msg: impl Into<String>) {
        let m = self.start();
        self.error(msg);
        self.bump_any();
        m.complete(self, SyntaxKind::ERROR);
    }

    /// Require `kind`. If present, consume it; otherwise emit a diagnostic
    /// at the expected position. The parser continues — the missing
    /// closer acts as a zero-width `ERROR` in the event stream
    /// (virtual-token insertion per spec §4.3).
    #[allow(dead_code)]
    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            self.error(format!("expected {kind:?}"));
            false
        }
    }

    /// Skip tokens until we hit one in `set`, an already-synchronising
    /// clause keyword, a `;`, or EOF. Skipped tokens are wrapped in a
    /// single `ERROR` node (spec §4.3 — synchronisation-set skip).
    pub(crate) fn recover_until(&mut self, set: TokenSet) {
        let stop = set.union(RECOVERY_STOP);
        if stop.contains(self.current()) || self.current() == SyntaxKind::EOF {
            return;
        }
        let m = self.start();
        while !stop.contains(self.current()) && self.current() != SyntaxKind::EOF {
            self.bump_any();
        }
        m.complete(self, SyntaxKind::ERROR);
    }

    /// Begin a new node. Returns a `Marker` that must be completed or
    /// abandoned before it is dropped.
    pub(crate) fn start(&mut self) -> Marker {
        let pos = self.events.len();
        self.events.push(Event::Start {
            kind: SyntaxKind::ERROR, // placeholder; replaced by complete()
            forward_parent: None,
        });
        Marker::new(pos)
    }

    pub(crate) fn into_events(self) -> Vec<Event> {
        self.events
    }
}

/// Advance `idx` past any leading trivia tokens.
fn skip_trivia(tokens: &[LexToken], mut idx: usize) -> usize {
    while idx < tokens.len() && tokens[idx].kind.is_trivia() {
        idx += 1;
    }
    idx
}

/// Clause-start keywords + statement terminators that always end recovery.
/// Any clause recovery set is implicitly unioned with this.
pub(crate) const RECOVERY_STOP: TokenSet = TokenSet::new(&[
    SyntaxKind::MATCH_KW,
    SyntaxKind::OPTIONAL_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::WITH_KW,
    SyntaxKind::RETURN_KW,
    SyntaxKind::CREATE_KW,
    SyntaxKind::MERGE_KW,
    SyntaxKind::SET_KW,
    SyntaxKind::REMOVE_KW,
    SyntaxKind::DELETE_KW,
    SyntaxKind::DETACH_KW,
    SyntaxKind::UNWIND_KW,
    SyntaxKind::CALL_KW,
    SyntaxKind::UNION_KW,
    SyntaxKind::SEMI,
]);

// ========================================================================
// Marker / CompletedMarker (drop-bomb protected)
// ========================================================================

/// A pending node-open. Must be either `complete`d or `abandon`ed before
/// it is dropped. The [`DropBomb`] panics in debug builds if dropped.
#[must_use = "Marker must be completed or abandoned"]
pub(crate) struct Marker {
    pos: u32,
    bomb: DropBomb,
}

impl Marker {
    fn new(pos: usize) -> Self {
        Self {
            pos: u32::try_from(pos).expect("event index fits u32"),
            bomb: DropBomb::new("Marker must be completed or abandoned"),
        }
    }

    /// Close the node with the given kind, yielding a `CompletedMarker`
    /// that can be used for Pratt-style re-parenting via [`CompletedMarker::precede`].
    pub(crate) fn complete(mut self, p: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
        self.bomb.defuse();
        let idx = self.pos as usize;
        match &mut p.events[idx] {
            Event::Start { kind: slot, .. } => *slot = kind,
            _ => unreachable!("marker index does not point to a Start event"),
        }
        p.events.push(Event::Finish);
        CompletedMarker {
            pos: self.pos,
            kind,
        }
    }

    /// Drop the speculative node. If no events were emitted after the
    /// marker's Start, we truncate; otherwise we tombstone the Start.
    #[allow(dead_code)]
    pub(crate) fn abandon(mut self, p: &mut Parser<'_>) {
        self.bomb.defuse();
        let idx = self.pos as usize;
        if idx + 1 == p.events.len() {
            p.events.pop();
        } else {
            p.events[idx] = Event::Abandoned;
        }
    }
}

/// A completed node handle. Can be retroactively re-parented via
/// [`CompletedMarker::precede`] — central to Pratt expression parsing.
#[derive(Debug, Copy, Clone)]
pub(crate) struct CompletedMarker {
    pos: u32,
    #[allow(dead_code)]
    kind: SyntaxKind,
}

impl CompletedMarker {
    /// Open a new outer node that will re-parent this completed node. The
    /// returned `Marker` must be completed; the outer node's Start/Finish
    /// will surround this node's Start/Finish in the emitted tree.
    pub(crate) fn precede(self, p: &mut Parser<'_>) -> Marker {
        let new_marker = p.start();
        // Link this completed Start's forward_parent to the new marker.
        let old_idx = self.pos as usize;
        let new_idx = new_marker.pos as usize;
        let delta = u32::try_from(new_idx - old_idx).expect("forward-parent delta fits u32");
        match &mut p.events[old_idx] {
            Event::Start { forward_parent, .. } => {
                debug_assert!(forward_parent.is_none(), "forward_parent already set");
                *forward_parent = Some(delta);
            }
            _ => unreachable!("CompletedMarker pos does not point to a Start"),
        }
        new_marker
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::SyntaxKind;

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

    #[test]
    fn root_kind_is_source_file() {
        let p = parse("MATCH (n) RETURN n");
        assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    }

    use proptest::prelude::*;

    proptest! {
        /// Property P17.3.2 — lossless CST over arbitrary UTF-8 input.
        #[test]
        fn lossless_on_arbitrary_utf8(s in ".*") {
            let p = parse(&s);
            prop_assert_eq!(p.syntax().to_string(), s);
        }

        /// Property P17.3.1 — parser totality: no panic, bounded time.
        #[test]
        fn parser_is_total_on_random_utf8(s in ".*") {
            let _ = parse(&s);
        }
    }
}
