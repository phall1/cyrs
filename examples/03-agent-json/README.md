# 03-agent-json — scripting via `cypher-agent`

`cypher-agent` is a JSON-over-stdio daemon (spec 0001 §15). One request
per line on stdin; one response per line on stdout. The agent supports
`parse`, `check`, `format`, `complete`, `hover`, `rewrite`, `plan`,
`explain`, `schema_set`, `schema_clear`, `shutdown`.

This example pipes four ops — `parse`, `check`, `format`, `shutdown` —
through the agent and diffs the result against a committed
`expected.jsonl`.

## Run

From the repo root, build once:

```sh
cargo build --release -p cypher-agent
```

Then from this directory:

```sh
./run.sh
```

That prints four JSON lines to stdout. To regression-check:

```sh
diff <(./run.sh) expected.jsonl && echo OK
```

## Contents

- `input.jsonl` — four requests, one per line.
- `expected.jsonl` — captured canonical response for each request.
- `run.sh` — pipes `input.jsonl` into the built `cypher-agent`.

## What to notice

- The `parse` response returns the CST round-trip string plus a list
  of parser-level syntax errors.
- The `check` response carries the full diagnostic list — code
  `E0011`, severity `error`, byte range, message. Codes are stable
  (spec §10).
- The `format` response is idempotent: `format(format(x)) == format(x)`.
- `shutdown` tells the agent to exit its read loop.

Request / response schemas are documented at the top of
`crates/cypher-agent/src/main.rs`.
