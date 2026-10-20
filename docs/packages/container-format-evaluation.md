# Xila package-container format evaluation

## Result

This is a reproducible host prototype, not a production parser selection. The corpus and writer prototype favor custom XPK on this one synthetic corpus by size, but there is not enough safety, embedded-resource or compatibility evidence to recommend a proprietary format. Keep restricted TAR/ustar and CPIO `newc` as candidates; before choosing, qualify a parser against Xila's polling VFS and complete malformed-input requirements. Do not infer target performance from this host experiment.

## Reproduction

From the repository root:

```sh
CARGO_TARGET_DIR=target/package-format-eval cargo test --manifest-path tools/package_format_eval/Cargo.toml
CARGO_TARGET_DIR=target/package-format-eval cargo run --release --manifest-path tools/package_format_eval/Cargo.toml -- evaluate --output target/package-format-eval --seed 402
```

The harness uses a generated deterministic corpus (seed 402): 1 MiB Wasm-like bytes, a TOML file, opaque PNG-like bytes, 100 tiny files, nested text, a path over 200 bytes spread across valid ustar prefix/name components, and a repeated-text compression case. Package output, `summary.csv`, and `summary.json` stay under the supplied output directory. Rust was `rustc 1.99.1` / `cargo 1.99.1`; pinned prototype dependencies are in `tools/package_format_eval/Cargo.lock`.

## Size results

The current host run produced:

| Format | Archive bytes | Manifest | Names | Headers | Padding | LZ4 framing | Stored payload |
|---|---:|---:|---:|---:|---:|---:|---:|
| CPIO newc | 1,102,956 | 3,061 | 2,972 | 11,880 | 13 | 40 | 1,084,990 |
| ustar TAR | 1,193,472 | 3,061 | 2,637 | 53,171 | 49,573 | 40 | 1,084,990 |
| custom XPK | 1,091,786 | 3,061 | 2,623 | 1,072 | 0 | 40 | 1,084,990 |

Each breakdown sums exactly to the artifact size. These numbers compare prototype encodings only; they are not representative package measurements. The LZ4 trial uses independent 4 KiB blocks, chooses raw storage when framed compression would not reduce size, and has a host roundtrip test. Only some corpus files compress, so this is not a general compression ratio claim.

## Reader and resource evidence

The probes wrap each artifact in a disk-like source with arbitrary short reads (including one-byte reads), then read through bounded chunks. The host `tar` crate's `Archive<R: Read>`/entry `Read` API works with this setup. The newc and custom probes use sequential bounded reads, but they are evaluation probes—not qualified production parsers. The newc writer uses `hadris-cpio`'s in-memory `FileTree`; that does not prove a streaming packer. The current no-default-features host check passes, but the evaluation crate is std/host-oriented and this is **not** an embedded no_std build.

No MCU-class linker/toolchain or hardware measurements were run. Parser/decoder flash delta, stack, peak RAM, allocation count, native extraction throughput/median/range, and polling-VFS integration are **unavailable**, not zero. No controlled three-run extraction benchmark is claimed; this harness measures deterministic artifact sizes and bounded read behavior only. `summary.json` records archive hashes, dependency versions, entry/read-buffer results, and `hardware_measurements: null` to represent unavailable measurements. Hashes are reproducibility checks, not cryptographic integrity/authentication.

## Known gaps and recommendation

- `oci-zero` and `tar-no-std` were not integrated; the host `tar` API is the only library TAR probe here.
- The malformed-input matrix is partial: custom truncation/traversal and ordinary package reads are tested, but duplicate/path normalization, links/special files, checksum corruption, comprehensive integer overflow, repeated/missing manifest, and malicious LZ4 output cases remain open.
- The probe checks metadata/order and consumes payloads; it does not yet extract each candidate into a sink and byte-compare every file.
- CPIO payload-read behavior and its documented exact-entry-sized convenience API still need an API-level no-allocation investigation; the handwritten probe is not a substitute.
- Postcard manifest encoding is measured, but bounded allocation-free decoding and schema ownership/conflict validation are not.
- The prototype's candidate readers do not prove compatibility with the VFS `Poll<Result<usize>>` cancellation/wakeup contract.

The modest XPK size win is insufficient evidence to own a permanent proprietary format. Next, extend qualification with a parser that obeys polling VFS semantics, run the adversarial corpus and sink roundtrips, inspect `oci-zero`/CPIO incremental APIs and establish native plus MCU resource measurements before making a production choice. Package installation, signatures/trust policy and download lifecycle remain outside scope (#308).
