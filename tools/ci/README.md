# CI performance and validation policy

Tracking: [#480](https://github.com/xila-project/core/issues/480).

## Coverage and trust boundaries

- PRs retain host tests, audio/native builds, host/guest Clippy, WASI application
  builds, ABI memory32 tests and doctests, and change-aware strict localization.
- Main/manual Check runs additionally execute documentation, host coverage and
  profiling, memory32/memory64 ABI coverage, Miri (including big-endian and 32-bit
  hosts), every shipped locale and the browser example. No tests are moved to a
  schedule by this change.
- Required check names remain `Guest checks`, `Format checks`, `Host checks
  (Clippy)`, `Host checks (Tests)`, and `Host checks (Audio, docs, build)`.
- Host workspace tests and audio/docs/native validation share one Cargo target
  directory on one runner. Every previous command and feature combination stays;
  independent validations continue after a preceding test failure. The audio/docs
  required status is a small result gate which rejects failed/skipped/missing host
  work. This avoids a second container/setup, font-generator build, bindgen/C
  rebuild and dependency archive. Trade-off: the two statuses complete together,
  and coverage waits for the combined host workload. Check runner-minutes and
  critical-path latency together before retaining this consolidation.
- A skipped path-selection ancestor must not disable extended validation. Extended
  jobs explicitly require successful direct tests and use `!cancelled()` to
  override GitHub's implicit status gate. A failed direct test still blocks them.
- PRs restore shared Rust/compiler caches read-only. Main saves dependency
  caches. Cache experiments are authenticated manual runs with separate cache
  namespaces; their branch-scoped caches are not promoted to main.
- Runtime container cancellation applies to superseded PRs, not main publication.
  Publication continues to smoke-test its own image before pushing it.

## Reproduce measurements

Use authenticated `gh` and Python 3 (standard library only):

```sh
python3 -m unittest discover -s tools/ci -v
python3 tools/ci/metrics.py \
  --run 39394164383 --run 39397807146 \
  --run 39397807254 --run 39394164333 \
  --output target/ci-metrics/baseline.json
```

The report records commit/event/attempt, workflow wall time (attempt start to last
non-skipped job completion), execution span, aggregate runner occupancy,
per-job/per-step duration and conclusion, archive exact-hit outcomes, Cargo `Compiling`/`Checking` line
counts, last sccache JSON statistics and Rust cache transfer sizes/times.
Post-job cache compression/save is included in job occupancy. These are not
GitHub billed/rounded minutes. A Cargo line is not a rustc invocation; sccache
counts cacheable requests, not Cargo-fresh units, linking or all build scripts.
Cache archive exact hit, Cargo freshness and compiler-cache hit are distinct.
Jobs and logs are attempt-scoped: rerunning a workflow does not count days of
inter-attempt delay or mix retained jobs from older attempts into the sample.

Transfers are measured from rust-cache's restore/save markers to its completion
marker. They include archive compression/decompression, not just network I/O.
Unknown sizes remain null; failed transfers are marked. Ordinary npm/BuildKit
transfers are not inferred from these Rust-cache markers; use their API step
durations/logs separately. Logs may expire, so retain the generated JSON with
benchmark results. Failed/cancelled/incomplete runs are not successful samples.

The setup action collects sccache server DEBUG messages privately because 0.18.0
logs write failures below WARN. Its post step publishes **only fixed error
category counts**, then removes the raw file. Never upload the raw file, HTTP
headers, signed URLs or environment dumps. Read-only PR write errors are expected;
they are not evidence of a main write failure. Category counts are log events,
not necessarily distinct compiler requests. An `other` count requires further
private investigation; do not guess the cause.

## Historical baseline, captured 2026-10-20

| Run | Conclusion | Wall minutes | Runner minutes |
| --- | --- | ---: | ---: |
| [main Check 39394164383](https://github.com/xila-project/core/actions/runs/39394164383) | success, but extended jobs skipped | 35.917 | 86.750 |
| [PR Check 39397807146](https://github.com/xila-project/core/actions/runs/39397807146) | success | 10.633 | 28.533 |
| [PR OCI 39397807254](https://github.com/xila-project/core/actions/runs/39397807254) | success | 12.217 | 12.167 |
| [main Pages 39394164333](https://github.com/xila-project/core/actions/runs/39394164333) | success | 33.000 | 32.850 |

The PR pair costs 40.700 runner-minutes. The main Check baseline is **incomplete
coverage**, so restored coverage/Miri must be accounted for in comparisons.
These are sampled different commits, not controlled cache experiments.

### Verified archive-compression waste

The development image at digest `3d9738f5f02afd9147be66b2c84908a56b24da68138ba03ca88885c973f11f09`
has GNU tar but no `zstd`. Historical logs show `tar ... -z`: the cache toolkit
silently falls back to gzip, spending most of the 90–135-second saves compressing
before uploading. The shared cache-tools action now installs Debian's signed
`zstd` package only if absent, before cache setup. Prefer shipping it in the
development image eventually. Installing it changes the cache compression/version;
expect a one-time cold cache, not an immediate hit on old gzip archives.

Local archive experiment (2026-10-20): same machine, same read-only 471-MiB
`target/x86_64-unknown-linux-gnu/debug/deps`, fixed image above, 3 sequential runs
per codec using GNU tar `--posix -z` versus `--posix --zstd`:

| Codec | Median archive creation | Median decompress/list | Archive bytes |
| --- | ---: | ---: | ---: |
| gzip | 13.632 s | 2.395 s | 164,013,877 |
| zstd | 0.523 s | 0.512 s | 139,085,977 |

This demonstrates compression work and archive-size reductions, **not** a measured
GitHub end-to-end speedup. Decompress/list is not extraction. Image package setup,
network, larger artifacts and cache warming must be included in Actions comparisons.

| Main Check job | Seconds | Cargo compiling / checking lines | Rust cache restore / save seconds | Archive bytes |
| --- | ---: | ---: | ---: | ---: |
| Localization | 2145 | 63 / 434 | 139.535 / exact hit, no save | 4,881,927,477 |
| ABI memory64 | 127 | 9 / 0 | miss / 5.251 | 30,476,260 |
| ABI memory32 | 127 | 9 / 0 | miss / 5.692 | 30,475,141 |
| Format | 100 | 0 / 0 | no Rust cache | — |
| Host tests | 599 | 429 / 9 | miss / 89.898 | 1,465,097,330 |
| Host audio/docs/build | 759 | 608 / 214 | miss / 111.198 | 1,669,678,957 |
| Guest | 842 | 554 / 74 | miss / 135.345 | 1,519,718,494 |
| Host Clippy work | 503 | 115 / 74 | miss / 59.675 (failed reservation) | unknown |
| Required Clippy gate | 3 | 0 / 0 | — | — |

Historical main sccache failures: host tests 1,278, audio/docs 1,594, guest 1,425,
localization 2,400. Existing logs do **not** contain their error categories.
Upstream [sccache#2821](https://github.com/mozilla/sccache/issues/2821) demonstrates
lost HTTP 429 writes, but this is a hypothesis for Xila, not an established cause.
The repository cache usage API reported 12,257,374,257 bytes / 331 entries during
this audit. The storage-limit endpoint returned HTTP 402 (payment-method
requirement), so neither the effective limit nor billing-related write rejection
is established. No production caches were deleted.

## Controlled cache experiments

`Cache benchmark` runs the same locked host workspace tests and font generation
on a fixed development-image digest with incremental compilation disabled.
Candidates run serially to avoid inter-candidate GHAC throttling. Default to one
candidate to bound compute and quota costs:

1. `dependencies`: rust-cache dependency artifacts, no compiler wrapper.
2. `combined`: dependency artifacts plus sccache GHAC (current architecture).
3. `sccache`: compiler cache only, no Cargo archive.
4. `workspace`: combined plus workspace artifacts, keyed by source SHA to avoid
   [rust-cache#348](https://github.com/Swatinem/rust-cache/issues/348). Checkout and
   generated-input mtimes may still defeat freshness; measure it.

After the workflow is available on main:

```sh
gh workflow run cache-benchmark.yml --ref YOUR_BRANCH \
  -f namespace=480-study-1 -f strategy=dependencies
# Collect the completed cold run, then repeat the identical ref/namespace for warm.
python3 tools/ci/metrics.py --run RUN_ID --output target/ci-metrics/cold.json
```

A fresh namespace isolates cold Actions caches, not the image's preinstalled
tools or external download mirrors. Use the **same commit**, image, strategy,
namespace, locale and successful workload for warm measurements. Report full
job time including cache saves, not only Cargo time. Each strategy/namespace can
consume gigabytes; use a small bounded study, record cache usage, and remove only
that study's experiment caches after results are retained. Do not raise quotas,
enable PR shared-cache writes, or introduce an external service to hide churn.

## Remaining investigation

- Confirm actual main GHAC write failure categories with new diagnostics before
  choosing retry/backoff, fewer writers, disk archives or a backend replacement.
- Benchmark total compile + archive + transfer cost before changing default
  cache strategy, enabling workspace caching, or changing dev/test debug info.
  Existing profile and nextest benchmark jobs remain opt-in; preserve doctests.
- Trace locale and LVGL dirty fingerprints on fixed inputs. The generator already
  skips identical ranges; LVGL watches the generated-font directory and local
  `lv_conf.h`. Do not suppress these watches or reuse locale artifacts blindly.
- Consolidate compatible feature/target builds and same-revision publication
  artifacts only after measured feature graphs demonstrate equivalent coverage.

References: [Cargo performance](https://doc.rust-lang.org/cargo/guide/build-performance.html),
[build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html),
[rust-cache](https://github.com/Swatinem/rust-cache),
[sccache GHAC](https://github.com/mozilla/sccache/blob/main/docs/GHA.md),
[GitHub caching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).
