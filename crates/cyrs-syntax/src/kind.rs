//! `SyntaxKind` — the canonical grammar reference (spec 0001 §4.4).
//!
//! Every grammar production and every token class has an entry here. The
//! enum is `repr(u16)` so it round-trips through `rowan::SyntaxKind`. New
//! kinds are appended at the end of their section; existing numeric values
//! are never reused, matching the diagnostic code policy (spec §10.2).
//!
//! The enum is partitioned into three zones:
//!
//! - Tokens (trivia, keywords, literals, punctuation, error markers).
//! - Nodes (clauses, patterns, expressions, etc.).
//! - Composite-only meta kinds (EOF, ROOT).
//!
//! The [`SyntaxKind::is_trivia`], [`SyntaxKind::is_keyword`],
//! [`SyntaxKind::is_punct`], [`SyntaxKind::is_literal`], and
//! [`SyntaxKind::is_node`] predicates are the primary consumers' lens on
//! these partitions.

#![allow(non_camel_case_types)]
// Every variant of `SyntaxKind` is a grammar-production name whose
// semantics live in spec §4.4 (tokens) and the `cypher.ungrammar` source
// (nodes).  Per-variant doc strings would duplicate those sources and
// add boilerplate without informational value; exempt the whole file
// from `missing_docs` rather than paper them all over.
#![allow(missing_docs)]

/// Every syntactic category in Cypher: tokens, nodes, and meta.
///
/// `repr(u16)` so it can be used directly as a rowan kind.
///
/// Marked `#[non_exhaustive]` per spec §4.4: consumers must use a
/// wildcard arm when matching, so the grammar can grow without
/// breaking every downstream match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[non_exhaustive]
pub enum SyntaxKind {
    // =====================================================================
    // Trivia (0..16)
    // =====================================================================
    WHITESPACE = 0,
    LINE_COMMENT,
    BLOCK_COMMENT,

    // =====================================================================
    // Identifiers & literals (16..48)
    // =====================================================================
    IDENT = 16,
    QUOTED_IDENT,
    INT_LITERAL,
    FLOAT_LITERAL,
    STRING_LITERAL,
    PARAM,
    BOOL_LITERAL,
    NULL_LITERAL,

    // =====================================================================
    // Punctuation (48..128)
    // =====================================================================
    L_PAREN = 48,
    R_PAREN,
    L_BRACK,
    R_BRACK,
    L_BRACE,
    R_BRACE,
    COMMA,
    SEMI,
    COLON,
    DOUBLE_COLON,
    DOT,
    DOT_DOT,
    PIPE,
    STAR,
    PLUS,
    MINUS,
    SLASH,
    PERCENT,
    CARET,
    EQ,
    NEQ,
    BANG_EQ,
    LT,
    LE,
    GT,
    GE,
    ARROW_R,
    ARROW_L,
    REGEX_EQ,
    DOLLAR,
    BANG,
    AMP,

    // =====================================================================
    // Keywords (128..320). `_KW` suffix to disambiguate from AST nodes.
    // =====================================================================
    MATCH_KW = 128,
    OPTIONAL_KW,
    WHERE_KW,
    WITH_KW,
    RETURN_KW,
    CREATE_KW,
    MERGE_KW,
    DELETE_KW,
    DETACH_KW,
    SET_KW,
    REMOVE_KW,
    UNWIND_KW,
    CALL_KW,
    YIELD_KW,
    ON_KW,
    AS_KW,
    AND_KW,
    OR_KW,
    XOR_KW,
    NOT_KW,
    IN_KW,
    IS_KW,
    NULL_KW,
    TRUE_KW,
    FALSE_KW,
    CASE_KW,
    WHEN_KW,
    THEN_KW,
    ELSE_KW,
    END_KW,
    ORDER_KW,
    BY_KW,
    ASC_KW,
    ASCENDING_KW,
    DESC_KW,
    DESCENDING_KW,
    SKIP_KW,
    LIMIT_KW,
    DISTINCT_KW,
    UNION_KW,
    ALL_KW,
    STARTS_KW,
    ENDS_KW,
    CONTAINS_KW,
    DIV_KW,
    MOD_KW,
    COUNT_KW,
    EXISTS_KW,
    SHORTESTPATH_KW,
    ALLSHORTESTPATHS_KW,
    // List-predicate keywords (cy-8x5). `ALL_KW` already exists above —
    // these three join the set so `ANY(x IN xs WHERE p)` etc. lex as
    // dedicated keyword tokens rather than identifiers. Shared across
    // GqlAligned and OpenCypherV9 (spec §9), so no dialect gate needed.
    ANY_KW,
    NONE_KW,
    SINGLE_KW,

    // GQL-distinct keyword (cy-8z3 / cy-0hj). `INSERT` is GQL's spelling for
    // node / edge creation (ISO/IEC 39075:2024 §13.4); openCypher uses
    // `CREATE`. The lexer recognises `INSERT` unconditionally — the
    // OpenCypherV9-vs-GqlAligned dialect gate lives in cyrs-sema. The two
    // qualifiers `NODE` / `EDGE` that follow `INSERT` stay as identifiers
    // (not keywords) so openCypher's `:NODE` / `:EDGE` label spellings keep
    // lexing as ordinary patterns; the parser recognises them contextually
    // after `INSERT` via `at_contextual`.
    INSERT_KW,

    // GQL-distinct keyword (cy-auh / cy-0hj). `EXCLUDE` is GQL's spelling
    // for projecting all bindings minus an explicit field list
    // (ISO/IEC 39075:2024 §14.13.4); openCypher has no equivalent. The
    // lexer recognises `EXCLUDE` unconditionally — the OpenCypherV9-vs-
    // GqlAligned dialect gate lives in cyrs-sema. Slot 182 is left empty
    // for the parallel `FILTER_KW` allocation; this variant pins to
    // raw 183 explicitly.
    EXCLUDE_KW = 183,

    // GQL post-projection row filter (cy-r50 / cy-0hj). `FILTER <expr>`
    // operates on the working table after projection (ISO/IEC 39075:2024
    // §14.10); openCypher's nearest paraphrase is `WITH ... WHERE`. The
    // lexer recognises `FILTER` unconditionally — dialect gate lives in
    // cyrs-sema. Pinned to raw 182 (slot that was reserved by parallel
    // EXCLUDE_KW landing).
    FILTER_KW = 182,

    // GQL path-selector keyword (cy-3mq / cy-0hj). `SHORTEST` introduces
    // a path search mode in a NAMED_PATTERN_PART (`p = ANY SHORTEST
    // (...)`, `p = SHORTEST 3 (...)`; ISO/IEC 39075:2024 §10.4.2).
    // Pinned to raw 184 (slot reserved for it during parallel work).
    SHORTEST_KW = 184,

    // GQL type-assertion keyword (cy-pnp / cy-0hj). `IS TYPED <type>` is
    // GQL's predicate form for run-time type assertions per ISO/IEC
    // 39075:2024 §6.5.2; the shorthand `::` lives at the punctuation
    // level (DOUBLE_COLON, already in the punctuation zone). Slot 184
    // reserved for parallel work; this variant pins to raw 185.
    TYPED_KW = 185,

    // GQL-distinct MATCH path-mode keywords (cy-q2g / cy-0hj). `REPEATABLE
    // ELEMENTS` and `DIFFERENT EDGES` (ISO/IEC 39075:2024 §10.6.3) sit
    // between `MATCH` and the first pattern and select the element-
    // uniqueness semantics of a variable-length path. All four words are
    // promoted to lexer-level keywords (verified empty in openCypher TCK
    // query bodies). The discriminator between the two surface forms is
    // the leading keyword child of the `PATH_MODE` node.
    REPEATABLE_KW = 186,
    ELEMENTS_KW = 187,
    DIFFERENT_KW = 188,
    EDGES_KW = 189,

    // =====================================================================
    // Syntax nodes (320..768)
    // =====================================================================

    // Roots
    SOURCE_FILE = 320,
    STATEMENT,

    // Clauses
    MATCH_CLAUSE,
    OPTIONAL_MATCH_CLAUSE,
    WHERE_CLAUSE,
    WITH_CLAUSE,
    RETURN_CLAUSE,
    CREATE_CLAUSE,
    MERGE_CLAUSE,
    SET_CLAUSE,
    REMOVE_CLAUSE,
    DELETE_CLAUSE,
    UNWIND_CLAUSE,
    CALL_CLAUSE,
    UNION_TAIL,
    MERGE_ACTION,

    // Return body
    RETURN_BODY,
    RETURN_ITEMS,
    RETURN_ITEM,
    ORDER_BY,
    ORDER_ITEM,
    SKIP_SUBCLAUSE,
    LIMIT_SUBCLAUSE,

    // Patterns
    PATTERN,
    PATTERN_PART,
    NAMED_PATTERN_PART,
    NODE_PATTERN,
    REL_PATTERN,
    REL_DETAIL,
    REL_LENGTH,
    LABEL_EXPR,
    REL_TYPE_EXPR,
    PROPERTY_MAP,

    // Set / remove
    SET_ITEM,
    REMOVE_ITEM,

    // Call
    YIELD_SUBCLAUSE,
    YIELD_ITEM,
    PROCEDURE_NAME,

    // Expressions
    BINARY_EXPR,
    UNARY_EXPR,
    POSTFIX_EXPR,
    LITERAL_EXPR,
    VAR_EXPR,
    PROP_ACCESS_EXPR,
    SUBSCRIPT_EXPR,
    LIST_LITERAL,
    MAP_LITERAL,
    MAP_PROJECTION,
    MAP_PROJECTION_ITEM,
    CASE_EXPR,
    CASE_WHEN_ARM,
    CASE_ELSE_ARM,
    FUNCTION_CALL,
    CALL_ARGS,
    PAREN_EXPR,
    LIST_COMPREHENSION,
    PATTERN_COMPREHENSION,
    PATTERN_PREDICATE,
    PARAM_EXPR,
    IS_NULL_EXPR,
    IN_EXPR,
    REGEX_MATCH_EXPR,
    STRING_OP_EXPR,

    // Shared
    NAME,
    ARG_LIST,

    // List indexing / slicing (cy-7s6.1). Kept at the end of the node zone
    // to preserve numeric stability of the already-assigned variants above.
    INDEX_EXPR,
    SLICE_EXPR,

    // List predicates: ANY / ALL / NONE / SINGLE (cy-8x5). Appended at the
    // end of the node zone for the same stability reason as cy-7s6.1. The
    // discriminant between the four is carried by the first keyword child
    // token (ANY_KW / ALL_KW / NONE_KW / SINGLE_KW) so a single node kind
    // covers all four surface forms.
    LIST_PREDICATE_EXPR,

    // shortestPath / allShortestPaths pattern (cy-b5b, spec §6.4 / §19
    // "shortest-path"). Wraps the inner path pattern; the discriminant
    // between `shortestPath(...)` and `allShortestPaths(...)` is the
    // first keyword child token (SHORTESTPATH_KW vs ALLSHORTESTPATHS_KW),
    // matching the LIST_PREDICATE_EXPR pattern above.
    SHORTEST_PATH_PATTERN,

    // GQL-distinct write clause (cy-8z3 / cy-0hj). `INSERT NODE` / `INSERT
    // EDGE` is GQL's spelling for `CREATE`-style data creation per
    // ISO/IEC 39075:2024 §13.4. Appended at the end of the node zone to
    // preserve the numeric stability of the prior variants. The NODE /
    // EDGE qualifier appears as an ordinary IDENT child token (contextual
    // recognition in the parser); the discriminant between the two forms
    // is the textual identifier under the `INSERT_KW`.
    INSERT_CLAUSE,

    // GQL post-projection row filter (cy-r50, ISO/IEC 39075:2024 §14.10).
    // Mirrors `WHERE_CLAUSE` shape (single Expr child) but lives at
    // clause level so it can chain after RETURN / WITH projection. Slot
    // 390 reserved for it by parallel work.
    FILTER_CLAUSE = 390,

    // GQL path selector (cy-3mq, ISO/IEC 39075:2024 §10.4.2). Sits
    // inside a NAMED_PATTERN_PART between the binder's `=` and the path
    // pattern; collapses `ANY SHORTEST`, `ALL SHORTEST`, and `SHORTEST k`
    // surface forms into one node whose discriminant is the leading
    // keyword child. Slot 393 reserved for it.
    PATH_SELECTOR = 393,

    // GQL `OPTIONAL CALL` qualifier (cy-tdl, ISO/IEC 39075:2024 §14.11.3).
    // Wraps the same body shape as `CALL_CLAUSE` but signals that the
    // procedure invocation is optional: the GQL semantics return the
    // empty multiset on failure rather than propagating an error. The
    // discriminant is the leading `OPTIONAL_KW` child token; the rest of
    // the node mirrors `CALL_CLAUSE`. Raw value 391: slot 390 is reserved
    // for `FILTER_CLAUSE` (parallel bead).
    OPTIONAL_CALL_CLAUSE = 391,

    // GQL-distinct return-projection trailer (cy-auh / cy-0hj). `RETURN n
    // EXCLUDE password` (ISO/IEC 39075:2024 §14.13.4) projects all of `n`'s
    // bindings minus the listed field names. Wrapped inside a `RETURN_CLAUSE`
    // (after the `RETURN_ITEMS` and before the `ORDER_BY` / `SKIP` / `LIMIT`
    // trailers). Slot 390 (`FILTER_CLAUSE`) reserved for parallel bead.
    RETURN_EXCLUDE = 392,

    // GQL type-assertion nodes (cy-pnp, ISO/IEC 39075:2024 §6.5.2 / §6.2).
    // `IS_TYPED_EXPR` wraps `<expr> IS [NOT] TYPED <TypeName>`; the
    // `IS NULL` path stays on `IS_NULL_EXPR` (no merger so HIR / sema can
    // discriminate without inspecting child tokens). `TYPE_CAST_EXPR`
    // wraps `<expr> :: <TypeName>` — the typed-value shorthand. Both
    // carry a `TYPE_NAME` child whose first IDENT is the head type name
    // (`INTEGER`, `STRING`, `ZONED`, …) followed by zero or more
    // continuation IDENTs (e.g. `ZONED DATETIME`).
    IS_TYPED_EXPR = 394,
    TYPE_CAST_EXPR,
    TYPE_NAME,

    // GQL-distinct MATCH path-mode (cy-q2g / cy-0hj). Wraps the two-token
    // pair `REPEATABLE ELEMENTS` or `DIFFERENT EDGES` (ISO/IEC 39075:2024
    // §10.6.3) that follows `MATCH` and selects the element-uniqueness
    // semantics of the patterns to its right.
    PATH_MODE = 397,

    // --- cy-51we typed literals + standalone INSERT ---
    // GQL typed temporal literal (cy-51we, ISO/IEC 39075:2024 §10.6).
    // Surface forms: `DATE 'YYYY-MM-DD'`, `DATETIME '…'`, `TIME '…'`,
    // `TIMESTAMP '…'`, `DURATION '…'`. The introducer (`DATE` / `DATETIME`
    // / `TIME` / `TIMESTAMP` / `DURATION`) is recognised CONTEXTUALLY at
    // the parser level — it stays as an `IDENT` token so that openCypher
    // queries can still spell `date` as a property key (e.g.
    // `CREATE (:A {date: date({year: 1910, month: 5, day: 6})})`) or as
    // a function-call identifier (`date(...)`). The discriminant between
    // the five surface forms is the textual content of the leading IDENT
    // child; downstream passes inspect the token text to classify.
    TYPED_TEMPORAL_LITERAL = 398,
    // --- end cy-51we ---

    // =====================================================================
    // Errors & EOF (768..1024)
    // =====================================================================
    ERROR = 768,
    EOF = 769,
    // Reserved for future expansion; additions keep numeric stability.
}

impl SyntaxKind {
    /// Convert a raw u16 back into a `SyntaxKind`. Returns `None` for
    /// values not in the enumeration.
    #[must_use]
    pub fn from_u16(raw: u16) -> Option<Self> {
        // Safety-free check: reject values outside any defined slot.
        // A hand-written match is the rowan-canonical pattern. Generated
        // here via an internal macro invocation to keep the match in sync
        // with the enum above.
        Some(match raw {
            0 => Self::WHITESPACE,
            1 => Self::LINE_COMMENT,
            2 => Self::BLOCK_COMMENT,

            16 => Self::IDENT,
            17 => Self::QUOTED_IDENT,
            18 => Self::INT_LITERAL,
            19 => Self::FLOAT_LITERAL,
            20 => Self::STRING_LITERAL,
            21 => Self::PARAM,
            22 => Self::BOOL_LITERAL,
            23 => Self::NULL_LITERAL,

            48 => Self::L_PAREN,
            49 => Self::R_PAREN,
            50 => Self::L_BRACK,
            51 => Self::R_BRACK,
            52 => Self::L_BRACE,
            53 => Self::R_BRACE,
            54 => Self::COMMA,
            55 => Self::SEMI,
            56 => Self::COLON,
            57 => Self::DOUBLE_COLON,
            58 => Self::DOT,
            59 => Self::DOT_DOT,
            60 => Self::PIPE,
            61 => Self::STAR,
            62 => Self::PLUS,
            63 => Self::MINUS,
            64 => Self::SLASH,
            65 => Self::PERCENT,
            66 => Self::CARET,
            67 => Self::EQ,
            68 => Self::NEQ,
            69 => Self::BANG_EQ,
            70 => Self::LT,
            71 => Self::LE,
            72 => Self::GT,
            73 => Self::GE,
            74 => Self::ARROW_R,
            75 => Self::ARROW_L,
            76 => Self::REGEX_EQ,
            77 => Self::DOLLAR,
            78 => Self::BANG,
            79 => Self::AMP,

            128 => Self::MATCH_KW,
            129 => Self::OPTIONAL_KW,
            130 => Self::WHERE_KW,
            131 => Self::WITH_KW,
            132 => Self::RETURN_KW,
            133 => Self::CREATE_KW,
            134 => Self::MERGE_KW,
            135 => Self::DELETE_KW,
            136 => Self::DETACH_KW,
            137 => Self::SET_KW,
            138 => Self::REMOVE_KW,
            139 => Self::UNWIND_KW,
            140 => Self::CALL_KW,
            141 => Self::YIELD_KW,
            142 => Self::ON_KW,
            143 => Self::AS_KW,
            144 => Self::AND_KW,
            145 => Self::OR_KW,
            146 => Self::XOR_KW,
            147 => Self::NOT_KW,
            148 => Self::IN_KW,
            149 => Self::IS_KW,
            150 => Self::NULL_KW,
            151 => Self::TRUE_KW,
            152 => Self::FALSE_KW,
            153 => Self::CASE_KW,
            154 => Self::WHEN_KW,
            155 => Self::THEN_KW,
            156 => Self::ELSE_KW,
            157 => Self::END_KW,
            158 => Self::ORDER_KW,
            159 => Self::BY_KW,
            160 => Self::ASC_KW,
            161 => Self::ASCENDING_KW,
            162 => Self::DESC_KW,
            163 => Self::DESCENDING_KW,
            164 => Self::SKIP_KW,
            165 => Self::LIMIT_KW,
            166 => Self::DISTINCT_KW,
            167 => Self::UNION_KW,
            168 => Self::ALL_KW,
            169 => Self::STARTS_KW,
            170 => Self::ENDS_KW,
            171 => Self::CONTAINS_KW,
            172 => Self::DIV_KW,
            173 => Self::MOD_KW,
            174 => Self::COUNT_KW,
            175 => Self::EXISTS_KW,
            176 => Self::SHORTESTPATH_KW,
            177 => Self::ALLSHORTESTPATHS_KW,
            178 => Self::ANY_KW,
            179 => Self::NONE_KW,
            180 => Self::SINGLE_KW,
            181 => Self::INSERT_KW,
            182 => Self::FILTER_KW,
            183 => Self::EXCLUDE_KW,
            184 => Self::SHORTEST_KW,
            185 => Self::TYPED_KW,
            186 => Self::REPEATABLE_KW,
            187 => Self::ELEMENTS_KW,
            188 => Self::DIFFERENT_KW,
            189 => Self::EDGES_KW,

            320 => Self::SOURCE_FILE,
            321 => Self::STATEMENT,
            322 => Self::MATCH_CLAUSE,
            323 => Self::OPTIONAL_MATCH_CLAUSE,
            324 => Self::WHERE_CLAUSE,
            325 => Self::WITH_CLAUSE,
            326 => Self::RETURN_CLAUSE,
            327 => Self::CREATE_CLAUSE,
            328 => Self::MERGE_CLAUSE,
            329 => Self::SET_CLAUSE,
            330 => Self::REMOVE_CLAUSE,
            331 => Self::DELETE_CLAUSE,
            332 => Self::UNWIND_CLAUSE,
            333 => Self::CALL_CLAUSE,
            334 => Self::UNION_TAIL,
            335 => Self::MERGE_ACTION,
            336 => Self::RETURN_BODY,
            337 => Self::RETURN_ITEMS,
            338 => Self::RETURN_ITEM,
            339 => Self::ORDER_BY,
            340 => Self::ORDER_ITEM,
            341 => Self::SKIP_SUBCLAUSE,
            342 => Self::LIMIT_SUBCLAUSE,
            343 => Self::PATTERN,
            344 => Self::PATTERN_PART,
            345 => Self::NAMED_PATTERN_PART,
            346 => Self::NODE_PATTERN,
            347 => Self::REL_PATTERN,
            348 => Self::REL_DETAIL,
            349 => Self::REL_LENGTH,
            350 => Self::LABEL_EXPR,
            351 => Self::REL_TYPE_EXPR,
            352 => Self::PROPERTY_MAP,
            353 => Self::SET_ITEM,
            354 => Self::REMOVE_ITEM,
            355 => Self::YIELD_SUBCLAUSE,
            356 => Self::YIELD_ITEM,
            357 => Self::PROCEDURE_NAME,
            358 => Self::BINARY_EXPR,
            359 => Self::UNARY_EXPR,
            360 => Self::POSTFIX_EXPR,
            361 => Self::LITERAL_EXPR,
            362 => Self::VAR_EXPR,
            363 => Self::PROP_ACCESS_EXPR,
            364 => Self::SUBSCRIPT_EXPR,
            365 => Self::LIST_LITERAL,
            366 => Self::MAP_LITERAL,
            367 => Self::MAP_PROJECTION,
            368 => Self::MAP_PROJECTION_ITEM,
            369 => Self::CASE_EXPR,
            370 => Self::CASE_WHEN_ARM,
            371 => Self::CASE_ELSE_ARM,
            372 => Self::FUNCTION_CALL,
            373 => Self::CALL_ARGS,
            374 => Self::PAREN_EXPR,
            375 => Self::LIST_COMPREHENSION,
            376 => Self::PATTERN_COMPREHENSION,
            377 => Self::PATTERN_PREDICATE,
            378 => Self::PARAM_EXPR,
            379 => Self::IS_NULL_EXPR,
            380 => Self::IN_EXPR,
            381 => Self::REGEX_MATCH_EXPR,
            382 => Self::STRING_OP_EXPR,
            383 => Self::NAME,
            384 => Self::ARG_LIST,
            385 => Self::INDEX_EXPR,
            386 => Self::SLICE_EXPR,
            387 => Self::LIST_PREDICATE_EXPR,
            388 => Self::SHORTEST_PATH_PATTERN,
            389 => Self::INSERT_CLAUSE,
            390 => Self::FILTER_CLAUSE,
            391 => Self::OPTIONAL_CALL_CLAUSE,
            393 => Self::PATH_SELECTOR,
            392 => Self::RETURN_EXCLUDE,
            394 => Self::IS_TYPED_EXPR,
            395 => Self::TYPE_CAST_EXPR,
            396 => Self::TYPE_NAME,
            397 => Self::PATH_MODE,
            398 => Self::TYPED_TEMPORAL_LITERAL,

            768 => Self::ERROR,
            769 => Self::EOF,

            _ => return None,
        })
    }

    // ---------- partition predicates ----------
    //
    // These are the consumers' lens on the zones laid out above
    // (spec §4.4). All are cheap range checks or `matches!` over a
    // handful of variants, and all are `const fn` so callers can use
    // them in const contexts (e.g. static tables).

    /// Returns `true` for the trivia zone (whitespace and comments).
    ///
    /// ```
    /// use cyrs_syntax::SyntaxKind;
    /// assert!(SyntaxKind::WHITESPACE.is_trivia());
    /// assert!(!SyntaxKind::IDENT.is_trivia());
    /// ```
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE | Self::LINE_COMMENT | Self::BLOCK_COMMENT
        )
    }

    /// Returns `true` for the keyword zone (`MATCH_KW..=EDGES_KW`).
    ///
    /// `EDGES_KW` is the current upper bound (cy-q2g path-mode).
    /// Internal slots may be unassigned — `from_u16` returns `None`
    /// for those gaps, so they never round-trip into a keyword.
    #[must_use]
    pub const fn is_keyword(self) -> bool {
        let k = self as u16;
        k >= Self::MATCH_KW as u16 && k <= Self::EDGES_KW as u16
    }

    /// Returns `true` for the punctuation zone (`L_PAREN..=AMP`).
    #[must_use]
    pub const fn is_punct(self) -> bool {
        let k = self as u16;
        k >= Self::L_PAREN as u16 && k <= Self::AMP as u16
    }

    /// Returns `true` for literal-shaped tokens: numeric, string, boolean,
    /// null, and parameter tokens (`$name` / `$0`).
    #[must_use]
    pub const fn is_literal(self) -> bool {
        matches!(
            self,
            Self::INT_LITERAL
                | Self::FLOAT_LITERAL
                | Self::STRING_LITERAL
                | Self::BOOL_LITERAL
                | Self::NULL_LITERAL
                | Self::PARAM
        )
    }

    /// Returns `true` for composite syntax nodes (clauses, patterns,
    /// expressions, etc. — every kind in `SOURCE_FILE..ERROR`).
    #[must_use]
    pub const fn is_node(self) -> bool {
        let k = self as u16;
        k >= Self::SOURCE_FILE as u16 && k < Self::ERROR as u16
    }

    /// Returns `true` for any kind that is a token rather than a node or
    /// meta sentinel. Equivalent to `!is_node() && !matches!(ERROR | EOF)`.
    #[must_use]
    pub const fn is_token(self) -> bool {
        (self as u16) < Self::SOURCE_FILE as u16
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind;

    /// Every kind defined in the enum must round-trip through `from_u16`.
    /// This is the canonical regression test for the grammar table.
    #[test]
    fn round_trip_known_kinds() {
        let known = [
            SyntaxKind::WHITESPACE,
            SyntaxKind::IDENT,
            SyntaxKind::MATCH_KW,
            SyntaxKind::ALLSHORTESTPATHS_KW,
            SyntaxKind::ANY_KW,
            SyntaxKind::NONE_KW,
            SyntaxKind::SINGLE_KW,
            SyntaxKind::SOURCE_FILE,
            SyntaxKind::LIST_PREDICATE_EXPR,
            SyntaxKind::SHORTEST_PATH_PATTERN,
            SyntaxKind::OPTIONAL_CALL_CLAUSE,
            SyntaxKind::REPEATABLE_KW,
            SyntaxKind::ELEMENTS_KW,
            SyntaxKind::DIFFERENT_KW,
            SyntaxKind::EDGES_KW,
            SyntaxKind::PATH_MODE,
            SyntaxKind::ERROR,
            SyntaxKind::EOF,
        ];
        for kind in known {
            let round = SyntaxKind::from_u16(kind as u16);
            assert_eq!(round, Some(kind), "round-trip failed for {kind:?}");
        }
    }

    #[test]
    fn partitions_are_disjoint() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(!SyntaxKind::WHITESPACE.is_keyword());
        assert!(SyntaxKind::MATCH_KW.is_keyword());
        assert!(!SyntaxKind::MATCH_KW.is_punct());
        assert!(SyntaxKind::L_PAREN.is_punct());
        assert!(SyntaxKind::SOURCE_FILE.is_node());
        assert!(!SyntaxKind::SOURCE_FILE.is_token());
        // cy-q2g: the four GQL path-mode keywords land inside the
        // keyword zone even with slots 184..=185 reserved for parallel
        // beads (`is_keyword` is a range check; the gap slots have no
        // enum variant so they cannot exist as values).
        assert!(SyntaxKind::REPEATABLE_KW.is_keyword());
        assert!(SyntaxKind::ELEMENTS_KW.is_keyword());
        assert!(SyntaxKind::DIFFERENT_KW.is_keyword());
        assert!(SyntaxKind::EDGES_KW.is_keyword());
        assert!(SyntaxKind::PATH_MODE.is_node());
    }
}
