# Changelog

All notable changes to the cyrs VS Code extension are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions track the cyrs workspace version.

## [0.0.1] — Unreleased

### Added

- Initial scaffold (bead cy-9w5).
- Language registration for `cypher` (`.cyp`, `.cypher`, `.gql`).
- TextMate grammar for keywords, literals, comments, labels, properties,
  parameters, and operators. Semantic tokens from `cypher-lsp` refine
  highlighting at runtime.
- Language-client wiring to the `cypher-lsp` binary over stdio.
  Discovery: `cyrs.server.path` setting, then `$CYPHER_LSP`, then
  `cypher-lsp` on `$PATH` (mirrors `demo/nvim/init.lua`).
- Settings for schema source / dialect / formatter options forwarded as
  `initializationOptions` (spec §14.3).
- `cyrs.restartServer` command.
- File-watcher synchronization for `*.cyp`, `*.cypher`, `*.gql`, `*.toml`.
