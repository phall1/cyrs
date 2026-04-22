# 02-project — multi-file workspace

Two `.cyp` files sharing one `schema.toml`. Demonstrates cross-file
label resolution (spec 0002, 0003): `Person` is declared once and
visible to every member.

Layout:

```
cypher-project.toml     # workspace manifest
schema.toml             # shared schema
queries/
    ok.cyp              # uses :Person — clean
    bad.cyp             # uses :Ghost — unknown-label error
```

## Run

From the repo root, build once:

```sh
cargo build --release -p cypher-cli
```

Then from this directory:

```sh
../../target/release/cypher check .
```

`cypher check .` walks up for a `cypher-project.toml`, loads every
member into one database, installs the schema, and runs analysis
per-file.

## Expected output

Stderr (ANSI elided):

```
error[E3001]: unknown label `:Ghost`
  ┌─ .../queries/bad.cyp:2:6
  │
2 │ MATCH (g:Ghost) RETURN g
  │      ^^^^^^^^^^

checked 2 files in project 'mini': 1 diagnostic
```

Exit code: `1` (one error-severity diagnostic). Drop `bad.cyp`, or add
`Ghost` to `schema.toml`, and the same command exits `0`.
