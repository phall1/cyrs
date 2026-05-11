# Cyrs Release Playbook

> **Audience:** the human operator.  Nothing in this playbook runs
> automatically — every step is explicit, every artifact is inspected
> before it ships.
>
> **Pre-reads:** spec 0001 §17.17 (release gating), §18 (versioning,
> MSRV), `docs/stability.md` (surface-by-surface stability contract).

Cyrs ships as nineteen crates (the `cyrs-*` layers + the `cyrs-lang`
meta-crate) from a single workspace.  Versions are bumped together:
every release is a coordinated roll across all publishable crates.
`cyrs-testkit`, `xtask`, and `tests/canary` carry `publish = false`
and are skipped.

The publishable set (19 crates):

```
cyrs-syntax  cyrs-ast       cyrs-hir      cyrs-sema      cyrs-schema
cyrs-project cyrs-diag      cyrs-plan     cyrs-fmt       cyrs-db
cyrs-lang-services           cyrs-tck     cyrs-lsp       cyrs-agent
cyrs-cli     cyrs-ffi       cyrs-py       cyrs-wasm      cyrs-lang
```

This document covers the three things the automated workflows
deliberately leave to the operator:

1. **Cutting a release** — when and how.
2. **Signing / SBOM / attestation** — what lands on the Release page.
3. **Failure recovery** — what to do when a step blows up.

Approval authority lives with the operator at every step.  No agent is
authorised to run `cargo publish`, `cargo release`, `git tag`, or
`git push --tags`.

---

## 0. Prerequisites (first-release-only setup)

Done once, before v0.1.0.  Skip for subsequent releases.

1. **crates.io token.**  Create a scoped publish token at
   <https://crates.io/settings/tokens> (scope:
   `publish-new` + `publish-update`).  Store as a repository secret
   named `CARGO_REGISTRY_TOKEN`.  The token is consumed only by the
   manual `publish-crates.yml` workflow; release-PR generation never
   publishes to crates.io.
2. **Protected branch `main`.**  Require PR reviews; enable the
   `ci / lint`, `ci / test (stable)`, and `semver-checks` status
   checks as required.
3. **Release-plz config.**  Default config is fine for pre-1.0; create
   `release-plz.toml` at repo root only when we need per-crate
   overrides (e.g. excluding `cyrs-testkit` from the publish pass,
   which Cargo already enforces via `publish = false`).
4. **`cargo-cyclonedx` installed on the CI runner.**  `sign-release.yml`
   runs `cargo install cargo-cyclonedx --locked` each invocation; no
   extra setup needed.
5. **Sigstore OIDC.**  GitHub Actions already mints the OIDC token
   `sign-release.yml` needs.  Cosign's Fulcio + Rekor endpoints are
   the defaults; no config in this repo.
6. **SLSA generator pinned.**  `sign-release.yml` calls
   `slsa-framework/slsa-github-generator/.github/workflows/
   generator_generic_slsa3.yml@v2.1.0`.  Bumping requires a signed
   PR.

---

## 1. Cutting a release

### 1.1 Pre-flight (run locally, on a clean `main`)

```sh
git checkout main
git pull --ff-only
cargo xtask gate          # fmt, clippy, test, doc, deny, recovery-budget
cargo xtask release       # spec §17.17 release gate
cargo xtask check-changelogs
```

If any step fails, fix on a branch, land via PR, retry.  Never cut a
release from a `main` with a red gate.

Manually sanity-check:

- [ ] Every crate's `CHANGELOG.md` under `[Unreleased]` is accurate.
      If a bead landed without a changelog line, add one now (on a
      branch).
- [ ] Root `CHANGELOG.md` captures workspace-level changes
      (toolchain, cross-cutting CI gates, MSRV).
- [ ] `docs/stability.md` matches reality — if a previously-`unstable`
      surface graduates to `stable`, say so.
- [ ] `cargo-semver-checks` is clean against `main`'s previous tip
      (the CI's `semver-checks` job covers this).

### 1.2 Cut the release PR

Go to **GitHub → Actions → release → Run workflow** and:

- **Branch:** `main`
- **`dry_run`:** `true` the first time — release-plz prints the
  planned version bumps + changelog pivots without opening a PR.
  Inspect the output.
- Re-run with **`dry_run`: `false`** to open the actual PR.

release-plz's PR will:

- Bump every publishable crate's `version` in its `Cargo.toml`.
- Pivot `[Unreleased]` → `[X.Y.Z] — YYYY-MM-DD` in every per-crate
  `CHANGELOG.md` and the root `CHANGELOG.md`.
- Add a fresh empty `[Unreleased]` section.

### 1.3 Review + merge

- Verify the version bump is sane (pre-1.0: patch for most changes;
  minor for new-feature-or-MSRV-bump; major never).
- Verify the changelog pivots are correct — release-plz groups by
  `### Added / Changed / …`; if an `### Added` entry ended up under
  `### Changed` (because of a misworded commit), fix in the PR
  branch.
- Merge via the GitHub UI (squash or merge commit — either is fine;
  keep-a-changelog doesn't care).

### 1.4 Create the GitHub Release

release-plz does not create a GitHub Release — that's the operator's
call.  From the repo's **Releases → Draft a new release**:

- **Tag:** `v<version>` (e.g. `v0.1.0`).  Create the tag on merge
  commit of the release PR.
- **Target:** the merge commit.
- **Release notes:** auto-generate from the commit log, then prepend
  the combined crates' changelog entries for readability.
- **Attach binaries:** upload `cypher` (built from `cyrs-cli`),
  `cypher-lsp` (built from `cyrs-lsp`), and `cypher-agent` (built
  from `cyrs-agent`) for at least `x86_64-linux`, `x86_64-macos`,
  `aarch64-macos`, `x86_64-windows`.  Build them with
  `cargo build --release -p cyrs-cli` (etc.) on a matching runner
  or use `cross` for the cross-platform legs.
- Click **Publish release**.

The `release:published` event triggers `sign-release.yml`
automatically — see §2.

### 1.4b Local dry-run validation (recommended)

Before flipping the publish workflow live, package the whole
workspace locally to confirm tarballs build and embed sane metadata:

```sh
cargo package --workspace --allow-dirty \
  --exclude cyrs-canary --exclude xtask \
  --exclude cyrs-testkit --exclude cyrs-py
```

`cyrs-py` is excluded from the verify pass because the `pyo3`
`extension-module` feature unresolves CPython symbols at link time
unless a libpython is on the link line; the wheel is built by
`maturin` in CI, not by bare `cargo publish`.  To confirm cyrs-py's
*manifest* is publish-clean, run:

```sh
cargo package -p cyrs-py --no-verify --allow-dirty
```

For the actual publish, use `--no-verify` for cyrs-py only:

```sh
cargo publish -p cyrs-py --no-verify
```

Note: `cargo publish --dry-run -p <crate>` for non-leaf crates
fails locally with "no matching package named `cyrs-syntax` found"
because dry-run still resolves dependencies against the live
crates.io index.  Use `cargo package --workspace` (above) instead;
it walks the workspace using path deps as published-version
substitutes.  The first real `cargo publish` of each crate populates
crates.io and unblocks subsequent dependents.

### 1.5 Publish to crates.io

crates.io publication is a manual GitHub Actions step once the release
PR is merged and tagged. Go to **GitHub → Actions → publish-crates →
Run workflow** and run with `dry_run: true` first. If the dry-run is
clean, re-run with `dry_run: false`.

The same command can be run from the operator's machine if needed:

```sh
# Set once per shell session:
export CARGO_REGISTRY_TOKEN=<redacted>

# Dry-run first. `cargo release` walks the workspace in dependency
# order and respects `publish = false` in cyrs-testkit / xtask.
cargo install cargo-release --locked
cargo release publish --workspace --no-confirm --dry-run

# When happy, drop --dry-run:
cargo release publish --workspace --no-confirm --execute
```

If `cargo-release` isn't available, publish by hand in dependency
order:

```sh
# Reads in spec §3.1 dependency order; each `cargo publish` blocks
# until crates.io indexes the new version.
cargo publish -p cyrs-syntax
cargo publish -p cyrs-diag
cargo publish -p cyrs-ast
cargo publish -p cyrs-schema
cargo publish -p cyrs-project
cargo publish -p cyrs-hir
cargo publish -p cyrs-sema
cargo publish -p cyrs-plan
cargo publish -p cyrs-fmt
cargo publish -p cyrs-db
cargo publish -p cyrs-lang-services
cargo publish -p cyrs-wasm
cargo publish -p cyrs-ffi
cargo publish -p cyrs-py --no-verify   # PyO3 extension; libpython unresolved at verify
cargo publish -p cyrs-tck
cargo publish -p cyrs-lsp
cargo publish -p cyrs-agent
cargo publish -p cyrs-cli
cargo publish -p cyrs-lang   # meta-crate goes last
```

Never publish `cyrs-testkit` (it is `publish = false`).

---

## 2. Signing, SBOM, and SLSA attestation

All three are produced by `.github/workflows/sign-release.yml` when the
GitHub Release is published.  No operator action required beyond
verifying the artifacts landed on the Release page.

### 2.1 What lands on the release

For every binary or dylib attached to the release:

- `<file>.sig` — detached cosign signature.
- `<file>.crt` — Fulcio-issued short-lived cert (chain of trust back
  to the Sigstore trust root).
- `<file>.intoto.jsonl` — SLSA v1 build-attestation attestation.

At the workspace level:

- `cyrs-<version>.cdx.json` — CycloneDX v1.5 SBOM for the whole
  workspace.

### 2.2 Verifying a signature (consumer side)

```sh
cosign verify-blob \
  --certificate <file>.crt \
  --signature   <file>.sig \
  --certificate-identity-regexp 'github.com/phall1/cyrs' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  <file>
```

A clean verify means: (1) the signature matches, (2) the Fulcio cert
chains to the Sigstore root, (3) the cert's OIDC identity maps to a
GitHub Actions run on `phall1/cyrs`, and (4) the signature is logged
in Rekor.

### 2.3 Verifying SLSA attestation

```sh
slsa-verifier verify-artifact \
  --attestation-path <file>.intoto.jsonl \
  --source-uri      github.com/phall1/cyrs \
  --source-tag      v<version> \
  <file>
```

---

## 3. Failure recovery

### 3.1 Release-plz PR opened with the wrong version

Close the PR without merging.  Fix the changelog wording (version
category is inferred from the `### Added` vs. `### Changed` headings)
and re-run `release.yml` with `dry_run=false`.

### 3.2 `cargo publish` failed mid-workspace

Crates.io publications are monotonic — a half-published workspace
leaves some crates at `X.Y.Z` and some at `X.Y.(Z-1)`.

1. Identify which crates succeeded via `cargo search cypher-*` (or the
   crates.io UI).
2. For the remaining crates, re-run `cargo publish -p <name>` in
   dependency order until the full set is at the new version.
3. If a crate cannot be published (name squat, renamed, etc.), yank
   the partial release:
   ```sh
   cargo yank --vers X.Y.Z -p <already-published-crate>
   ```
   Then bump to `X.Y.(Z+1)` via a fresh release PR; do NOT republish
   the same version number.

### 3.3 Cosign keyless signing failed

OIDC hiccups are transient.  Re-run `sign-release.yml` via
`workflow_dispatch` against the release tag.  The workflow is
idempotent — it overwrites existing `.sig` / `.crt` assets via
`gh release upload --clobber`.

### 3.4 SBOM / SLSA step failed

SBOM is safe to re-run; it regenerates the whole workspace manifest.
The SLSA generator is stricter — if its build step fails, file an
issue against `slsa-framework/slsa-github-generator` with the run log
and re-run the signing job without the SLSA leg via the manual
trigger.  The operator accepts the attestation gap for that release
and documents it in the root CHANGELOG's `### Security` section.

### 3.5 Release must be withdrawn entirely

1. `cargo yank --vers X.Y.Z -p <each-crate>` for every crate.
2. On GitHub: mark the Release as "Pre-release" and add a
   **YANKED** banner to the body.
3. Do not delete the tag (downstream consumers may still resolve it
   transitively); leave it pointed at the merge commit for audit.
4. Open a post-mortem issue.  Bead the follow-up work.

---

## 4. Approval matrix

| Action                              | Who approves       |
| ----------------------------------- | ------------------ |
| Run `release.yml` dry-run           | Operator           |
| Run `release.yml` non-dry-run       | Operator           |
| Merge release PR                    | Operator + one reviewer |
| Tag + publish GitHub Release        | Operator           |
| Run `cargo publish` / `cargo release` | Operator (only)  |
| Yank a published version            | Operator           |
| Bump MSRV (`rust-toolchain.toml`)   | Operator + spec amendment |
| Add a new crate to the publish set  | Operator + spec amendment |

Agents may *prepare* any of the above (open PRs, write changelog
entries, dry-run workflows) but must not execute the approved-only
actions without explicit operator instruction (AGENTS.md §0, §12).

---

## 5. Cross-references

- `docs/stability.md` — what is and is not stable across versions.
- `docs/specs/0001-cypher-frontend.md` §17.17 — the release gate.
- `docs/specs/0001-cypher-frontend.md` §18 — versioning + MSRV
  policy.
- `.github/workflows/release.yml` — the (inert) release PR workflow.
- `.github/workflows/sign-release.yml` — signing + SBOM + SLSA.
- `CHANGELOG.md` — workspace-level notes.
- `crates/*/CHANGELOG.md` — per-crate release notes.

---

*Playbook owner: the operator.  Last revised: 2026-04-22 (cy-zgz).*
