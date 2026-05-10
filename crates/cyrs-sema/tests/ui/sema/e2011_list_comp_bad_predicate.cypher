// E2011: list-comprehension WHERE predicate must be Bool (spec §7.4 /
// §19). A string literal as predicate is non-Bool and triggers the
// same diagnostic class used elsewhere for boolean-operator misuse.
// cy-5gh.
RETURN [x IN [1, 2, 3] WHERE 'maybe']
