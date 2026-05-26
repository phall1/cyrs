# Concept: syntax

The text-facing layer of the compiler front-end. Two things live here:
the **lexer** (tokens) and the **recovering parser** (lossless concrete
syntax tree).

**Crate:** [`cyrs-syntax`](../../crates/cyrs-syntax) — `SyntaxKind`,
`Parse`, `SyntaxNode`, `SyntaxToken`.
**Spec section:** [0001 §3.1, §4](../specs/0001-cypher-frontend.md).

## What goes in, what comes out

| In | Out |
| -- | --- |
| `&str` source text (Cypher or GQL) | `Parse` — root `SyntaxNode` (rowan green/red tree) + a list of `SyntaxError`s |

The output is *lossless*: every byte of the input — whitespace, comments,
malformed runs — is preserved as a node or trivia attached to a node.
The tree can be serialised back to the original string verbatim. This
property is what makes refactoring, format-on-save, and editor selection
ranges sound.

## How it stays useful on broken input

The parser is **recovering**. When it hits an unexpected token it
synthesises an error node and continues from a known follow-set rather
than bailing. One malformed clause does not erase the rest of the tree.
This is the editor-grade property: a half-typed query still produces a
usable structure with diagnostics anchored to spans.

## Where the grammar lives

The typed-AST layer above ([`ast`](./hir.md#related-typed-ast)) is
**code-generated** from
[`crates/cyrs-syntax/cypher.ungrammar`](../../crates/cyrs-syntax/cypher.ungrammar).
Editing the grammar regenerates the accessors; no hand-written `as_node`
boilerplate lives in the tree.

## When to reach for this layer

Choose `syntax` (or `syntax` + `ast`) when:

- The original source text matters: formatters, syntax highlighters,
  refactor tools that need to preserve comments and layout.
- A consumer wants to walk a tree without paying for name resolution or
  type checking — for example, a quick "does this parse?" probe.
- Building tree-sitter parity or another editor-side parser is out of
  scope; cyrs is the authoritative tree.

A consumer that needs resolved names should consume [`hir`](./hir.md)
instead. Consuming `syntax` alone and re-implementing scopes on top of
it is the most common embedder mistake.

## Performance notes

The tree is `rowan`-based (the same library rust-analyzer uses): nodes
are persistent and shared, so re-parses on small edits are cheap. The
incremental DB in [`services`](./services.md) builds on this — it
re-runs analysis only on the changed subtrees.

## Related

- Typed AST wrappers: [`cyrs-ast`](../../crates/cyrs-ast), generated
  from the same `cypher.ungrammar`. Strips trivia, keeps structure.
- Tree-sitter grammar: [`tree-sitter-cypher/`](../../tree-sitter-cypher),
  kept in lock-step with the Rust parser via
  `cargo xtask tree-sitter-parity`.
- Next layer down: [`hir`](./hir.md).
