// dialect: OpenCypherV9
//
// E4020: `SESSION SET …` is a GQL-only top-level statement category
// (ISO/IEC 39075:2024 §14.15; spec §0 amendment 2026-05-19 cy-5e3f).
// openCypher v9 has no equivalent; the dialect-gate pass fires
// `GATE_SESSION_SET` for every occurrence under `OpenCypherV9`.
//
// Bead cy-lp3y wired the HIR + sema follow-up to the parser/AST that
// landed in cy-9kzx. This fixture pins the `GqlAligned`-only
// gate at the user-visible surface.
SESSION SET GRAPH CURRENT_GRAPH
