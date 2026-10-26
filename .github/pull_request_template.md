## Summary

<!-- What changed, and why? Keep the main behavior and scope clear. -->

## Related issues

<!-- e.g. Closes #123, Fixes #456, or Relates to #789. Remove if none. -->

## Changes

<!-- Highlight important API, behavior, architecture, or user-visible changes. -->

## Validation

<!-- Check only tests you actually ran. CI provides additional coverage after opening the PR. -->

- [ ] Host build/check: `mise run check:host`
- [ ] Wasm guest check: `mise run check:guest`
- [ ] Host/Wasm Clippy: `mise run clippy:all`
- [ ] Workspace tests: `mise run test:workspace`
- [ ] Formatting / relevant localization checks
- [ ] Target-specific or focused tests (describe below)

**Commands, results, and evidence:**

<!-- Include test output summaries, a CI link, screenshots/UI smoke evidence, or before/after measurements as appropriate. -->

**Not tested / limitations:**

<!-- State untested platforms and missing toolchains; host/Wasm checks do not prove embedded hardware support. -->

## Compatibility, resources, and risks

<!-- Describe relevant no_std / feature flags, API/ABI or migration effects, permissions/security, and RAM/flash/stack/heap impact. Write "Not applicable" when appropriate. -->

## Checklist

- [ ] Changes are scoped and follow the owning crate's conventions.
- [ ] Tests and documentation are updated where needed, or the omission is explained.
- [ ] No secrets, unintended generated files, or local-only `docs/superpowers/` material are included.
