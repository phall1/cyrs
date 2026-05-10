# 0008 — Span vs `TextRange` ergonomics for embedders

**Severity:** low
**Discovered:** error.rs adapter sketching

## Problem

The embedder's legacy parser uses a flat `Span { start: u32, end: u32 }`
(byte offsets into the source string). cyrs uses `text_size::TextRange`
(also byte offsets, but typed via `text_size::TextSize`).

Both are the same data, but the conversion is repetitive:

```rust
let span = Span::new(range.start().into(), range.end().into());
```

…in dozens of places.

## Proposed shape

`cyrs-syntax` could expose a `TextRange` extension trait or a `pub
fn as_byte_range(r: TextRange) -> std::ops::Range<usize>` so embedders
can interop with their own Span/Range types in one call.

Alternatively, document the recommended conversion idiom in a
README or `cyrs-syntax::span` module doc.

## Why it matters for the embedder

Cosmetic — but ~50 sites in the embedder construct legacy Spans today.
Each will need a converter when stage 2 lands.
