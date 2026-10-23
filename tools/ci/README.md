# CI workflows

`ci.yml` runs the primary quality, host, WASM, ABI, and localization checks.
The five validation jobs run independently; `Required` is the branch-protection
status. On PRs, localization checks select affected catalogs and always expand
to all locales for shared localization/build infrastructure. Main runs all seven
locales.

`extended.yml` runs coverage, ABI coverage/Miri, and profiling checks nightly or
manually. Version-tag image publication calls this workflow on the tag commit and
requires its exact SHA to pass. Main image publication waits for successful
primary CI and image smoke tests, but not extended validation.

`runtime.yml` never publishes PR images. `pages.yml` rebuilds documentation,
deterministic filesystem images, and all locale demos after successful main CI.
`benchmarks.yml` is manual-only and contains Cargo profile and test-runner
experiments. The former cache-benchmark workflow was already removed from the
working tree and is not recreated here.

## Caching

Build jobs use the pinned `rust-cache` action for Cargo artifacts and sccache for
compiler outputs. PRs restore caches read-only; main can save shared caches.
Runtime image builds retain BuildKit/GitHub Actions caching. Quality checks do
not initialize Rust build caches.

## Local checks

```sh
python3 -m unittest discover -s tools/ci -v
actionlint .github/workflows/*.yml
MISE_ENV=ci mise run ci:host
MISE_ENV=ci mise run ci:wasm
MISE_ENV=ci CI_ABI_MEMORY=memory_32 mise run ci:abi
MISE_ENV=ci CI_LOCALES="en fr es de pt-BR it pl" mise run ci:localization
MISE_ENV=ci mise run ci:coverage
MISE_ENV=ci mise run pages:build
```

The extended Miri and Pages tasks require their pinned nightly/tool dependencies;
runtime image smoke tests require Docker.
