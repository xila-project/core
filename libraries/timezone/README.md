# Civil time and IANA zones

`xila-timezone` converts Unix UTC instants and local civil fields without changing
system UTC or monotonic time. It is a reusable `no_std` library for applications
such as Clock, separate from the time subsystem. Static conversion needs no
allocator or system calls. Locale, language, 12/24-hour formatting, preference
selection and persistence belong to callers; there is no implicit current zone.

## Selecting data

The default build contains UTC only. Enable just the required zones:

```toml
timezone = { package = "xila-timezone", path = "libraries/timezone", default-features = false, features = ["zone-europe-paris"] }
```

| Feature | Additional ID |
| --- | --- |
| none | `UTC` (`Etc/UTC` is an alias) |
| `zone-europe-paris` | `Europe/Paris` |
| `zone-america-new-york` | `America/New_York` |
| `zone-asia-kathmandu` | `Asia/Kathmandu` |
| `zone-asia-kolkata` | `Asia/Kolkata` |
| `zone-australia-lord-howe` | `Australia/Lord_Howe` |
| `bundled` | All five named zones above, plus UTC |
| `runtime-clock` | Optional adapter to `time::Manager`; adds no zones |

`AVAILABLE_ZONE_IDS` lists compiled registry IDs. Lookup is case-sensitive.
Unknown or omitted IDs return `Error::UnavailableZone`; falling back to
`Zone::UTC` requires an explicit caller decision. No host zoneinfo, environment,
browser locale, filesystem or network lookup occurs. The mandatory kernel clock
has no dependency on this library or its database. The root `xila` facade also
exposes optional `timezone` and `timezone-clock` features.

For another zone, enable `iana-data` and use one Jiff static per unique ID:

```rust
static LONDON: timezone::ZoneRules = jiff::tz::get!("Europe/London");
let zone = timezone::Zone::from_static(&LONDON)?;
```

The caller needs the same Jiff dependency with `default-features = false` and
`features = ["static"]`. This adds only that zone's compiled data, without
expanding the lookup registry. `from_static` derives the ID from the rules;
unnamed/fixed rules are rejected (Jiff's canonical UTC remains named).
Externally supplied rules have caller-owned version/provenance tracking.

## Conversion and ambiguity

`Zone::utc_to_local` takes **signed Unix nanoseconds**. The result provides civil
fields, the original instant, the zone ID, the current offset in seconds,
abbreviation and DST status. Historical offsets can include seconds;
abbreviations are informational and are not unique IDs or translated labels.
Nanoseconds and pre-epoch instants are preserved.

```rust
use timezone::{CivilDateTime, LocalResolution, Zone};

let paris = Zone::get("Europe/Paris")?;
let local = paris.utc_to_local(1_719_792_000_000_000_000)?;
assert_eq!(local.offset_seconds(), 7200);

let requested = CivilDateTime::new(2024, 10, 27, 2, 30, 0, 0).unwrap();
assert!(matches!(paris.local_to_utc(requested)?,
    LocalResolution::Ambiguous { earlier, later } if earlier < later));
```

Local-to-UTC conversion never chooses an offset implicitly:

- `Unique(nanoseconds)`: exactly one instant.
- `Ambiguous { earlier, later }`: two actual UTC instants, ordered by time;
  callers must choose explicitly or reject the input.
- `Nonexistent { before_offset_seconds, after_offset_seconds }`: a spring gap
  or other forward offset change. No UTC instant exists; the library does not
  shift the requested local time. Reject it or ask for a different input.

`CivilDateTime` is Jiff's validated civil type. Invalid calendar input fails its
constructor/parser. The conversion engine supports civil years -9999 through
9999; the Unix timestamp endpoints reserve enough margin for any supported
zone offset. Exact UTC bounds are Jiff's `Timestamp::MIN`/`MAX`, narrower than the
kernel's i128 representation. Out-of-range conversions return `Error::OutOfRange`
without saturation. Unix/POSIX leap-second conventions follow the UTC source.
IANA history is best supported after 1970; older local mean times reflect the
available database evidence, not a guarantee of historical completeness.

With `runtime-clock`, `zone.now(&manager)` first reads valid system UTC, returning
`Error::Clock(time::Error::Unsynchronized)` or another original clock error when
unavailable. Civil range errors remain `OutOfRange`. Supplying raw nanoseconds
directly assumes the caller already established epoch validity. Neither API
sets the clock, applies a timezone offset to UTC, or changes elapsed deadlines.

## Database and updates

The pinned generator uses Jiff **0.2.38** and `jiff-tzdb` **0.1.9**, carrying
**IANA 2026e**. `DATABASE_VERSION` exposes the build dependency's actual version;
it is `None` for UTC-only builds. The full database is used on the **build host**
by the procedural macro; only selected static rules enter the target binary.
Build-host tooling uses `std`, while the target library remains core-only.
No build reads the host's installed timezone database.

To update rules:

1. Review upstream Jiff and IANA changes; update the exact Jiff and database pins
   together and regenerate the root and probe lockfiles with Cargo.
2. Confirm the resolved macro/database versions and `DATABASE_VERSION`; update
   the version assertion in the conversion tests deliberately.
3. Run the feature matrix, historical offsets, gaps/folds, non-hour changes and
   year-2200 regression tests; remeasure the linked probes below.
4. Rebuild/redeploy firmware and Wasm/native artifacts. There is no runtime
   database download or automatic OS update propagation.

TZif recurring footer rules cover future dates under the **current** legislation.
They cannot predict future legal changes. Persist zone IDs in a caller's settings
rather than saving a current offset or a boot-relative UTC correction (#467).

## Dependency evaluation

Measured on 2026-10-26 with Rust 1.99.0, Linux x86_64. The comparison used the same
five zones (UTC, Paris, New York, Kathmandu, Lord Howe), variable timestamp/zone
selection, an offset lookup export, and `opt-level="s"`, LTO, one codegen unit,
stripping and aborting panics. Native figures include a common `std` caller and
allocation counter. Wasm figures are final stripped modules, not rlib sizes.

| Candidate/configuration | Target `no_std` check | Native text / data bytes | Wasm bytes | Paris, July 2200 |
| --- | --- | ---: | ---: | ---: |
| Jiff 0.2.38, defaults off, static subset | Pass | 310,703 / 11,776 | 51,902 | +02:00 |
| chrono-tz 0.10.4 + chrono 0.4.45, defaults off | Pass | 1,119,272 / 1,025,704 | 896,722 | +01:00 |
| Same chrono-tz, regex-filtered to five zones | Pass | 293,228 / 38,664 | 30,084 | +01:00 |
| time-tz 3.0.0-rc.6.0.0, default database | Fails (`std` required) | 1,293,899 / 1,542,856 | 1,607,747 | +01:00 |

All four host offset probes allocated zero during conversion and built for
Wasm. `no_std` checks used `thumbv7em-none-eabihf`; time-tz's manifest/source
unconditionally uses `std`. Chrono's filtered build used
`CHRONO_TZ_TIMEZONE_FILTER='^(UTC|Europe/Paris|America/New_York|Asia/Kathmandu|Australia/Lord_Howe)$'`.
The tested chrono database is 2025b. Its and time-tz's generated transition tables
stop at 2100, returning their final offsets in 2200, unlike Jiff's recurring
rules. The release-candidate time-tz version is also an explicit maintenance
consideration. These results describe these versions/configurations.

Filtered chrono is smaller in this Wasm offset-only probe. Jiff was selected for
checked civil conversion, explicit ambiguity, current pinned data and correct
recurring future rules, accepting that size tradeoff. Defaults disable Jiff's
system detection, runtime databases, `std`, `alloc`, JS and serialization features.

## Linked footprint and validation

Reproduce the **actual library** probes (both conversion directions and metadata):

```sh
bash libraries/timezone/check.sh
bash tools/timezone_probe/host.sh
bash tools/timezone_probe/check.sh thumbv7em-none-eabi
bash tools/timezone_probe/check.sh wasm32-unknown-unknown
```

Install those Rust targets, Clippy/rustfmt, Python 3 and a native linker first;
`llvm-size` enables Cortex-M section reporting. CI's `ci:timezone` task installs
its toolchain/targets and runs the feature matrix and both allocator-free links.
The probe has its own locked workspace and no production dependency on a full DB.
To compare the alternative crates, use the configurations above with the same
five-zone offset-only export; its workload differs from this full-API probe.

Final x86_64 release executable sections, bytes (includes the `std` caller):

| Library profile | text | data | bss |
| --- | ---: | ---: | ---: |
| Absent (control) | 272,826 | 9,464 | 2,322 |
| UTC only | 275,104 | 9,488 | 4,106 |
| UTC + Paris | 308,019 | 11,160 | 2,298 |
| Bundled six IDs | 319,565 | 12,608 | 1,570 |

All host profiles reported **zero conversion allocations**. On x86_64, `Zone`
is 24 bytes, `LocalTime` 80 bytes and `LocalResolution` 48 bytes. Native PIE
relocation sections, libc state, linker layout and allocation-counter overhead
make the native `data`/`bss` columns unsuitable as embedded RAM estimates.

Final linked bare-metal Cortex-M (`thumbv7em-none-eabi`) text + read-only data:

| Library profile | Code + read-only bytes | Writable data | BSS |
| --- | ---: | ---: | ---: |
| Absent | 46 | 0 | 0 |
| UTC only | 1,963 | 0 | 0 |
| UTC + Paris | 18,300 | 0 | 0 |
| Bundled six IDs | 29,088 | 0 | 0 |

Final linked `wasm32-unknown-unknown` release modules, bytes:

| Library profile | Code section | Data section | Whole module | Declared initial memory |
| --- | ---: | ---: | ---: | ---: |
| Absent | 48 | 0 | 113 | 1,048,576 |
| UTC only | 1,297 | 12 | 1,418 | 1,114,112 |
| UTC + Paris | 15,019 | 5,474 | 20,686 | 1,114,112 |
| Bundled six IDs | 15,377 | 16,062 | 31,633 | 1,114,112 |

Section sizes include encoding overhead. Wasm memory includes Rust's reserved
1 MiB stack and page rounding; it is **not live heap usage**. Both Cortex-M and
Wasm probes link with **no allocator**; a positive control introducing a `Box`
is required to fail. This establishes allocation freedom for the retained API
paths. Peak stack usage and hardware execution were not measured.

The native feature matrix tests UTC-only, each zone, bundled and the optional
clock adapter; it checks pure conversion for WebAssembly, WASI and Cortex-M.
Browser runtime compilation with the optional clock facade also passed.
ESP32-S3 compilation was attempted but the environment lacks the Xtensa
`core`/toolchain (E0463). Cortex-M/Wasm checks do not establish ESP32 hardware
correctness or footprint. No UI, timezone preference persistence or Clock app
behavior is changed by this library.
