; tree-sitter-cypher — language injections.
;
; Cypher does not embed other languages syntactically in v1. The only
; near-candidate is the `LOAD CSV` clause (which references an external
; CSV URL but carries no embedded text), and `LOAD CSV` is banned by
; spec §9.3 anyway.
;
; This file is intentionally a stub so editors that auto-load
; `queries/injections.scm` don't warn about a missing file. Add entries
; here if a future spec revision introduces an embedded sub-language
; (e.g. APOC Cypher scripts as string literals).
