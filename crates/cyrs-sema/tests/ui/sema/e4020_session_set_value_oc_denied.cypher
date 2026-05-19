// dialect: OpenCypherV9
//
// E4020: `SESSION SET VALUE [IF NOT EXISTS] $param = <expr>` is the
// parameter-binding form of the GQL-only `SESSION SET …` statement
// (ISO §14.15). Bead cy-lp3y.
SESSION SET VALUE IF NOT EXISTS $foo = {a: 1}
