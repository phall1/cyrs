// cy-z0x8: GQL `ORDER BY x NULLS` sort spec with the trailing
// `FIRST` / `LAST` discriminator omitted.  Surfaces E0104
// (`EXPECTED_FIRST_OR_LAST_AFTER_NULLS`); the parser still produces a
// recoverable CST so the rest of the program is analysed.
// ISO/IEC 39075:2024 §14.13.6 `nullOrdering`.
MATCH (n:Person)
RETURN n.age AS a
ORDER BY a NULLS
