# Shared verification migration

The canonical `verification` crate now lives in [Xila-Project/package_manager](https://github.com/xila-project/package_manager/tree/main/crates/verification).

Core pins its dependency to immutable commit `f5e3f0b323a79a80d106ce2b88b77bfa8667d71b`; this preserves the same public SHA-256 and strict Ed25519 Rust interfaces without maintaining a second cryptographic implementation. The standalone verifier probe resolves that same revision and retains its allocation-free target-link checks.

To update, change the pinned commit in both Cargo manifests, regenerate **both** Cargo.lock files using Cargo, and rerun `mise run ci:verification` plus the representative firmware build checks. This extraction must not be merged until the complete Core validation passes.

The original in-repository source was migrated unchanged in shared [pull request #3](https://github.com/xila-project/package_manager/pull/3).
