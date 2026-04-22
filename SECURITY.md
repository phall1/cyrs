# Security policy

## Supported versions

| Version  | Supported |
| -------- | --------- |
| `0.1.x`  | yes       |
| `< 0.1`  | no        |

cyrs is pre-1.0; the only supported line is the current `0.1.x`. Older
tags receive no backports.

## Reporting a vulnerability

Report privately. Do not open a public issue for a suspected security
bug.

- **Email:** `security@example.invalid` <!-- TODO(operator): replace with real security contact -->
- Include: affected version / commit, reproduction (query text, schema,
  dialect), observed behaviour, impact assessment.
- A GPG key, if you want one, will be listed here once the operator
  wires up the real address.

We will acknowledge receipt within **72 hours** and commit to a fix or
documented mitigation within **30 days** for issues rated critical or
high. Lower-severity issues are triaged on the normal bead track.

## Scope

In scope:

- Panics, infinite loops, or pathological resource use in `cypher-syntax`,
  `cypher-sema`, `cypher-hir`, `cypher-plan`, or `cypher-fmt` triggered
  by malicious input.
- Memory-safety bugs in any `unsafe` block, notably in the C FFI layer
  of `cypher-ffi` (spec 0004).
- Denial-of-service through crafted schemas loaded by `cypher-project`.
- Sandbox escapes in the WASM build (`cypher-wasm`, spec 0004).

Out of scope:

- The behaviour of downstream executors. cyrs is a front-end — it
  parses, analyses, plans. It does not execute queries. Running an
  untrusted query against a storage engine is the consumer's problem
  (spec §1.3 N1, §12.5).
- Issues in third-party dependencies that do not reach cyrs's public
  API; report those upstream.

## Fuzz infrastructure

Fuzz targets for lexer, parser, formatter, sema, and plan live under
[`fuzz/`](./fuzz/) and run in CI (5-minute PR gate, 24-hour nightly).
OSS-Fuzz integration is tracked in spec §17.4. Reporters are welcome
to run the targets locally (`cargo xtask fuzz <target>`) and include
the reproducer in the report.

## Acknowledgements

Reporters who follow this policy will be credited in the release notes
for the fix, unless they ask otherwise.
