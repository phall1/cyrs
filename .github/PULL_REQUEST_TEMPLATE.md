<!--
Thanks for opening a PR. Please fill out the sections below.
See CONTRIBUTING.md for the full workflow.
-->

## Summary

<!-- 1–3 sentences. What does this change? Why? -->

## Bead

<!-- e.g. cy-abc. Use `br show <id>` to copy the title. -->

Implements: `cy-___`

## Spec section

<!-- e.g. spec 0001 §7.3. Every non-trivial change cites one. -->

## Test plan

- [ ] Added / updated tests (unit / snapshot / proptest / compiletest)
- [ ] `cargo xtask gate` green locally
- [ ] TCK baseline regenerated (if pass-rate shifts)
- [ ] Snapshots reviewed (`cargo insta review`)
- [ ] UI fixtures added for any new diagnostic codes

## Checks

- [ ] Bead ID cited in every commit message
- [ ] Spec section cited in at least one commit message
- [ ] No new domain names (AGENTS.md §2.C2 denylist)
- [ ] No new workspace crate without a spec amendment (AGENTS.md §3.1, §2.C3)
- [ ] No new public enum variant without `#[non_exhaustive]` (cy-2i9)
- [ ] No `--no-verify`, `git reset --hard`, or force-push

## Notes for reviewers

<!-- Anything non-obvious: design tradeoff, pre-existing bug surfaced, follow-up bead filed. -->
