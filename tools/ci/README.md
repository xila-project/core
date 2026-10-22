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
- A skipped path-selection ancestor must not disable extended validation. Extended
  jobs explicitly require successful direct tests and use `!cancelled()` to
  override GitHub's implicit status gate. A failed direct test still blocks them.
- PRs restore shared Rust/compiler caches read-only. Main saves dependency
  caches. Cache experiments are authenticated manual runs with separate cache
  namespaces; their branch-scoped caches are not promoted to main.
- Runtime container cancellation applies to superseded PRs, not main publication.
  Publication continues to smoke-test its own image before pushing it.
- CI measurement/policy scripts are excluded from the runtime Docker context and
  runtime path filters. They are not Cargo workspace members or runtime inputs;
  changing a metrics script must not invalidate `COPY . .` and rebuild the OS.
  Their Python tests run in Format; product tools (`fs_image`, profiling and future
  non-`ci` tools) still select host checks. Workflow/action/Cargo configuration
  changes continue selecting the broader Rust validation graph.

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
not necessarily distinct compiler requests. `startup_*` categories identify the
backend capability probe that can put a nominally READ_WRITE sccache backend into
read-only mode. An `other` count requires further private investigation; do not
guess the cause.

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

Implementation observations (same Rust source, CI-only revisions; not controlled
cold/warm cache candidates):

| Run | Cache state | Conclusion | Wall / runner minutes |
| --- | --- | --- | ---: |
| [First fixes 39503077630](https://github.com/xila-project/core/actions/runs/39503077630) | existing gzip dependency caches | success | 8.100 / 25.367 |
| [zstd bootstrap 39504766790](https://github.com/xila-project/core/actions/runs/39504766790) | all Rust archive misses after compression-version change | success | 13.500 / 33.617 |
| [main after #481 39821789352](https://github.com/xila-project/core/actions/runs/39821789352) | warmed caches; localization ended at the old 30-minute budget during pt-BR | cancelled (coverage/Miri succeeded) | 30.333 / 67.600 |

The zstd bootstrap run emitted 429 Cargo compile lines in Host tests and 518 in the
separate audio/docs/build job; respective runner times were 413s and 574s. These totals
are a baseline for the stacked host-artifact consolidation experiment, not a
warm-cache performance claim. Its ABI-only job took 63s, setup 6s; its 1316/1663
host-test/audio sccache write events classified as **read-only**, as intended on
PRs. No main write-cause conclusion follows from these PR counters.

### Verified archive-compression waste

The development image at digest `3d9738f5f02afd9147be66b2c84908a56b24da68138ba03ca88885c973f11f09`
has GNU tar but no `zstd`. Historical logs show `tar ... -z`: the cache toolkit
silently falls back to gzip, spending most of the 90–135-second saves compressing
before uploading. The shared cache-tools action originally installed Debian's
signed `zstd` package only if absent, before cache setup. Development-environment
PR #1 now ships and validates zstd in amd64/arm64 images, so the normal path is a
cheap presence check. Installing it changes the cache compression/version; expect
a one-time cold cache, not an immediate hit on old gzip archives.
If package installation is unavailable, warn and retain the toolkit's gzip
fallback rather than skipping actual validation because an optional optimization
failed. Image inclusion removes this extra network setup entirely.

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
localization 2,400. Those older logs do **not** contain error categories. The first
post-#481 main/manual sample [39821789352](https://github.com/xila-project/core/actions/runs/39821789352)
has categorized evidence: 4,298 HTTP-429/rate-limited write log events and 51
409/AlreadyExists events across its jobs. There were no cache read errors or
timeouts. This confirms **write throttling is a real Xila cause**; already-exists
is the expected duplicate-key race. The sample's localization was canceled by a
newer main push, so it is not a successful full-workflow benchmark.

Upstream [sccache#2821](https://github.com/mozilla/sccache/issues/2821) traces this
to sccache v0.18.0 GHAC storage installing no retry layer for temporary HTTP429
errors. It remains open as of 2026-10-22; v0.18.0 is the latest release and no
upstream fix is available. A separate report's proposed exponential retry layer
recovered most 429s but did not honor `Retry-After` (observed asks 1–21 seconds).
We do not vendor/patch sccache or increase cache writers without a justified,
measured maintenance/security case. Serializing this workflow's cache experiments
avoids cross-candidate contention; normal Check jobs remain parallel.
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

On main, completed four-candidate cold study [39504796318 attempt 1](https://github.com/xila-project/core/actions/runs/39504796318/attempts/1)
used identical source and fixed image, serial jobs, fresh namespace `480-a930fc10`:

| Candidate | Cold job seconds | Rust archive | sccache outcome |
| --- | ---: | --- | --- |
| dependencies only | 442 | 1.304 GB saved (17.3s save) | disabled |
| combined | 502 | 1.304 GB saved (25.9s save) | 770 writes, 248 rate-limited, 166 AlreadyExists |
| sccache only | 651 | none | 472 writes, 632 rate-limited, 36 AlreadyExists |
| workspace + sccache | 474 | save failed: cache-key reservation collision | 1,551 write errors; invalid for warm comparison |

The rerun is not a complete warm matrix: only `combined` restored an exact
1.304-GB archive (23.3s restore, `Cache up-to-date`, 55 Cargo compile lines,
396s job); dependencies and workspace were cold again. The sccache-only result did
not establish a comparable warm archive. Do not compare the aggregate rerun's
30.233 runner-minutes with its cold 34.483 as a strategy result. This does
establish that one exact combined warm restore produced far fewer Cargo compile
lines, but the job still took 396s and transfer consumed 23.3s. It does not justify
changing cache defaults: cold dependency-only was fastest, and only one warm
combined candidate restored. A future study should first address workspace save
collisions and compare a repeated exact dependency-only cache.

The `workflow_dispatch` experiment is on main and can reproduce additional runs:

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
  The cargo-test runner comparison uses `--tests` for the timed unit/integration
  phase, then a common explicit doctest phase for both runners. Otherwise the
  default cargo-test arm would be charged for doctests twice.
- Trace locale and LVGL dirty fingerprints on fixed inputs. The generator already
  skips identical ranges; LVGL watches the generated-font directory and local
  `lv_conf.h`. Do not suppress these watches or reuse locale artifacts blindly.
- Consolidate compatible feature/target builds and same-revision publication
  artifacts only after measured feature graphs demonstrate equivalent coverage.

### Verified LVGL fingerprint reproduction

A disposable host-only probe using the actual pinned LVGL revision, Core fonts,
Node dependencies and Cargo 1.99.0 reproduced the following (single-run
observations, not end-to-end CI measurements). The forced-regeneration row
deliberately removed the ignored generated-font directory/ranges lock to model a
fresh isolated CI checkout. It does **not** mean a warm workspace regenerates
fonts after the existing fast path succeeds.

| Inputs | Cargo check elapsed | LVGL result |
| --- | ---: | --- |
| Identical inputs, existing generator fast path (ranges lock present) | 0.09 s | fresh; outputs and mtimes unchanged |
| Fresh-checkout equivalent: generate seven byte-identical font files | 42.60 s | dirty, 502 C objects rebuilt |
| Touch byte-identical copied `lv_conf.h` | 42.19 s | dirty, C build and binding generation rerun |
| Restore byte-identical fonts/config with original timestamps | 0.26 s | fresh |

SHA-256 snapshots established identical generated bytes. The existing generator
fast path correctly avoids conversion and preserves outputs when its ranges lock
is present. The expensive case is a **new isolated runner**: generated fonts and
their ignored ranges lock are source-tree files outside the rust-cache target
archive, so each job has to create them before compiling. Trace: `Dirty
lvgl_rust_sys ...: the file generated_fonts has changed`; the **bindgen crate**
remained fresh while binding generation executed inside LVGL's build script.
Both fonts and config need attention; caching fonts alone cannot fix checkout
config mtimes. Do not blindly backdate files. Treat this as a generated-input
reuse follow-up, not a request to weaken LVGL watches or remove the generator's
fast path. A candidate cache must key the full font source bytes, generator code,
converter/package version, Cargo.lock, locale and fallback; verify the ranges lock
and every expected output; and preserve/restore timestamps only for byte-identical
trusted inputs. The current ranges lock records glyph ranges, not a full input
digest, so it cannot safely serve as that complete cache key by itself.

Locked unit graphs also show a featureless host LVGL build dependency for Wasm's
raw-binding metadata alongside the `color_depth_32,shadcn-theme` rendering unit.
Guest metadata consumers unnecessarily pay for a full native C build. An upstream
bindings-only mode merits explicit ABI/target validation; simply unifying features
would not remove different host/target contexts safely. The three bindgen versions
belong to distinct locked dependencies (littlefs 0.70.1, LVGL 0.72.1, ABI 0.73.2), not
identical duplicate units. No speculative dependency/profile changes are made here.

References: [Cargo performance](https://doc.rust-lang.org/cargo/guide/build-performance.html),
[build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html),
[rust-cache](https://github.com/Swatinem/rust-cache),
[sccache GHAC](https://github.com/mozilla/sccache/blob/main/docs/GHA.md),
[GitHub caching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).
