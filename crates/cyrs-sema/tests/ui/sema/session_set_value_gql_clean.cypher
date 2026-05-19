// cy-lp3y: `SESSION SET VALUE …` is in scope for `GqlAligned`
// (ISO/IEC 39075:2024 §14.15). Sema validates the syntactic shape
// only — no parameter-name resolution, no value typing.
SESSION SET VALUE IF NOT EXISTS $foo = {a: 1}
