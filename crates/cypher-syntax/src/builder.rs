//! Green-tree builder: consumes the parser's [`Event`] stream and produces
//! a lossless [`rowan`] green tree (spec 0001 §4.4).
//!
//! # Role
//!
//! The parser in [`crate::parser`] never mutates a tree directly. Instead
//! it emits a flat [`Event`] stream that this module replays into
//! [`rowan::GreenNodeBuilder`]. Separating the two stages enables:
//!
//! - **Pratt associativity fixes**: a `Start` event can be retroactively
//!   re-parented via [`crate::parser::CompletedMarker::precede`], which
//!   sets a forward-parent link the builder resolves here.
//! - **Trivia interleaving**: whitespace and comments are skipped by the
//!   parser cursor but re-inserted around `Token` events so the resulting
//!   tree is byte-identical to the input (spec §4.4 losslessness).
//! - **Deferred error lifting**: syntax errors are collected alongside the
//!   tree and returned as a separate list for later diagnostic lowering
//!   (spec §10; `cypher-syntax` does not depend on `cypher-diag` — see
//!   §3.1).
//!
//! # Invariant
//!
//! For every input `s`: `parse(s).syntax().to_string() == s`. The
//! property test `lossless_on_arbitrary_utf8` in [`crate::parser`]
//! enforces this over arbitrary UTF-8.

use rowan::{GreenNode, GreenNodeBuilder};

use crate::{
    SyntaxKind,
    lexer::LexToken,
    parser::{Event, SyntaxError},
};

/// Wraps [`rowan::GreenNodeBuilder`] with an error accumulator and trivia
/// interleaving. Consumes the parser event stream plus the original token
/// slice and produces a green tree whose textual content equals the input
/// byte-for-byte (spec §4.4).
#[derive(Debug, Default)]
pub(crate) struct SyntaxTreeBuilder {
    inner: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
}

impl SyntaxTreeBuilder {
    /// Replay `events` against `tokens`, returning the finished green tree
    /// plus every accumulated [`SyntaxError`] in emission order.
    ///
    /// The algorithm mirrors rust-analyzer's event processor:
    ///
    /// 1. Resolve forward-parent chains. A `Start` with `forward_parent =
    ///    Some(rel)` means "re-parent me inside the node started `rel`
    ///    events ahead". Such chains are walked outer-to-inner so the
    ///    structural parent is opened first.
    /// 2. Interleave trivia. Each significant `Token` event is preceded
    ///    by any pending trivia tokens from the source stream, so the
    ///    tree reproduces the input verbatim.
    /// 3. Fold trailing trivia inside the final `Finish` event so the
    ///    rowan tree has a single root node (EOF trivia would otherwise
    ///    dangle outside the source-file node).
    pub(crate) fn build(
        tokens: &[LexToken],
        mut events: Vec<Event>,
    ) -> (GreenNode, Vec<SyntaxError>) {
        let mut builder = SyntaxTreeBuilder::default();
        let mut tok_idx: usize = 0;

        // Pre-allocated scratch buffer for resolved forward-parent chains;
        // cleared and reused per Start event to avoid per-node allocation.
        let mut forward_parents: Vec<SyntaxKind> = Vec::new();

        // Identify the last Finish event. Trailing trivia at EOF must be
        // flushed INSIDE the enclosing node before that Finish so the
        // rowan tree keeps a single root.
        let last_finish_idx = events
            .iter()
            .rposition(|e| matches!(e, Event::Finish))
            .unwrap_or(usize::MAX);

        for i in 0..events.len() {
            match std::mem::replace(&mut events[i], Event::tombstone()) {
                Event::Start {
                    kind,
                    forward_parent,
                } => {
                    // Walk forward-parent chain to collect outer wrappers.
                    forward_parents.clear();
                    forward_parents.push(kind);
                    let mut idx = i;
                    let mut fp = forward_parent;
                    while let Some(rel) = fp {
                        idx += rel as usize;
                        match std::mem::replace(&mut events[idx], Event::tombstone()) {
                            Event::Start {
                                kind: outer_kind,
                                forward_parent: next_fp,
                            } => {
                                forward_parents.push(outer_kind);
                                fp = next_fp;
                            }
                            _ => unreachable!("forward parent at event {idx} must be a Start node"),
                        }
                    }
                    // Start outer-to-inner. Trivia is deliberately NOT
                    // flushed here: spec §4.1 binds trivia to the
                    // FOLLOWING significant token, so pending trivia must
                    // wait for the next `Token` event before it is
                    // emitted — otherwise it would be absorbed by a
                    // freshly-opened inner node.
                    for &k in forward_parents.iter().rev() {
                        builder.inner.start_node(rowan::SyntaxKind(k as u16));
                    }
                }
                Event::Finish => {
                    if i == last_finish_idx {
                        // Flush trailing trivia INSIDE the root-closing
                        // node so rowan sees a single sub-tree. Spec
                        // §4.1 permits trailing trivia to attach to the
                        // preceding significant token at EOF; placing it
                        // under the source-file root still satisfies
                        // §4.4 losslessness.
                        while tok_idx < tokens.len() {
                            builder.inner.token(
                                rowan::SyntaxKind(tokens[tok_idx].kind as u16),
                                &tokens[tok_idx].text,
                            );
                            tok_idx += 1;
                        }
                    }
                    builder.inner.finish_node();
                }
                Event::Token { kind } => {
                    // Emit all preceding trivia, then the significant
                    // token itself.
                    while tok_idx < tokens.len() && tokens[tok_idx].kind.is_trivia() {
                        builder.inner.token(
                            rowan::SyntaxKind(tokens[tok_idx].kind as u16),
                            &tokens[tok_idx].text,
                        );
                        tok_idx += 1;
                    }
                    debug_assert!(
                        tok_idx < tokens.len(),
                        "Token event past end of token stream (kind={kind:?})"
                    );
                    debug_assert_eq!(
                        tokens[tok_idx].kind, kind,
                        "Token event kind mismatch: expected {:?}, saw {:?}",
                        tokens[tok_idx].kind, kind
                    );
                    builder.inner.token(
                        rowan::SyntaxKind(tokens[tok_idx].kind as u16),
                        &tokens[tok_idx].text,
                    );
                    tok_idx += 1;
                }
                Event::Error { code, msg, offset } => {
                    builder.errors.push(SyntaxError {
                        code,
                        message: msg,
                        offset,
                    });
                }
                Event::Abandoned => {}
            }
        }

        // All trailing trivia was folded inside the final Finish event's
        // enclosing node above; nothing should remain here. In the
        // degenerate case of an empty event stream (e.g. empty input
        // where source_file() never emits Finish — which it does), this
        // loop is a harmless no-op.
        debug_assert!(
            tok_idx == tokens.len(),
            "{} trivia token(s) unflushed after build",
            tokens.len() - tok_idx
        );

        (builder.inner.finish(), builder.errors)
    }
}
