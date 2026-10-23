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

Validation, Pages and benchmark build jobs use `jdx/mr-boxington-action@v1`. It reuses mbx
from `PATH`, installing it only on runners where it is absent; there is no
second project-managed mbx installation. The mise Cargo wrapper enables mbx
when `XILA_MBX=1`, including nested mise tasks and explicit nightly selections.
Quality checks do not opt in. No workflow retains sccache or `Swatinem/rust-cache`.

WASM, profiling and benchmarks use the action's `target` payload, including Cargo downloads.
Host (which tests independent workspaces), ABI, coverage, localization, and
Pages use `objects` to cover multiple workspaces and custom target directories.
Objects payloads do not carry Cargo registry downloads. Locale-specific target
directories remain separate, while content-addressed objects share one job
bundle. Each job family and ABI memory mode has a separate cache generation
(isolating both primary keys and fallback restore prefixes), and
the action keys name the selected Rust toolchain.

PRs (including forks), tags, schedules, and Pages `workflow_run` builds are
restore-only; successful default-branch pushes can save. To seed or refresh
the extended-validation, Pages and benchmark caches, manually dispatch those workflows
on `main`: only main-branch dispatches are permitted to save. Their scheduled
and `workflow_run` executions then reuse those entries; the action does not
support automatic saving for these event types. Runtime image builds retain
BuildKit/GitHub Actions caching.
No cache timing or benchmark workflow is added by this migration.

## Local checks

`mise.toml` contains reusable tasks. CI orchestration (`ci:*`) and
Pages builds are defined completely in `mise.ci.toml`,
alongside their tools and package dependencies; select them with `MISE_ENV=ci`.

All task names use `<namespace>:<command>`. Developer-only formatting,
translation, interactive Cargo, coverage and pre-commit tasks live in
`mise.development.toml` (for example `format:rust` and `dev:before-commit`).
CI has its own check-only `ci:format-rust` and `ci:format-toml` tasks and does
not load the development toolset. Shared commands include `check:host`,
`clippy:guest`, `docs:all`, `image:native`, and `deps:npm`. Old names are not aliases.

CI native package declarations live in `mise.ci.toml`, not workflow APT commands.
CI build tasks depend on `ci:packages-linker` (Clang/Mold) or `ci:packages-host`
(also GUI/audio libraries). These setup tasks explicitly apply mise bootstrap
packages with the appropriate environment filter. Formatting and quality tasks
have no package-setup dependency, and local development behavior is unchanged.
Benchmarks run package setup, including GNU time, before their timed commands.

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

The extended Miri and Pages demo tasks declare their pinned nightly toolchain,
components and targets in `tools.rust`; mise installs them on demand, without a
separate nightly setup task. Runtime image smoke tests require Docker.
