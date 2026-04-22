# OSS-Fuzz onboarding scaffolding (cy-h07)

Artifacts ready for upstream submission to
[google/oss-fuzz](https://github.com/google/oss-fuzz). Nothing here is
consumed by cyrs's own CI — our PR-gate fuzz-smoke and nightly 24 h runs
invoke `cargo fuzz` directly (see `.github/workflows/{ci,fuzz-nightly}.yml`).

| File | Purpose |
|------|---------|
| `project.yaml` | OSS-Fuzz project manifest: contacts, sanitizers, engine, architectures. |
| `Dockerfile` | Builder image — clones cyrs at HEAD inside `base-builder-rust`. |
| `build.sh` | Builds every fuzz target, copies binaries + dicts + zipped seed corpora into `$OUT`. |

## Submitting

Operator-gated. Do NOT submit without approval.

The upstream flow, per
<https://google.github.io/oss-fuzz/getting-started/new-project-guide/>:

1. Fork <https://github.com/google/oss-fuzz>.
2. Create `projects/cyrs/` in the fork, mirroring this directory's
   contents (`project.yaml`, `Dockerfile`, `build.sh`).
3. Run a local sanity pass:
   ```sh
   python infra/helper.py build_image cyrs
   python infra/helper.py build_fuzzers --sanitizer address cyrs
   python infra/helper.py check_build cyrs
   python infra/helper.py run_fuzzer cyrs fuzz_parser
   ```
4. Open a PR against `google/oss-fuzz:master` with the cyrs maintainer
   (currently `phallsignup@gmail.com`) as the primary contact.
5. After merge, ClusterFuzz takes ~24 h to start producing coverage +
   crash reports. The initial seed corpus is whatever ships in
   `fuzz/corpus/<target>/` at submission time.

## Syncing back from OSS-Fuzz

Once live, ClusterFuzz continuously grows the corpus. To pull the
current corpus for local re-runs:

```sh
# Requires gsutil + a Google account with read access to the public
# ClusterFuzz bucket. Replace <target> with the target name.
gsutil -m rsync -d \
    gs://cyrs-corpus.clusterfuzz-external.appspot.com/libFuzzer/cyrs_<target>/ \
    fuzz/corpus/<target>/
```

Crash reproducers live under
`gs://cyrs-corpus.clusterfuzz-external.appspot.com/libFuzzer/cyrs_<target>/fuzzer_stats`
and are linked from the ClusterFuzz dashboard for each report. See
`docs/fuzz-runbook.md` for the triage playbook.
