# xila-time-zone

Time zone and daylight saving time conversion for Xila, based on POSIX TZ rule
strings. The crate is `no_std`, performs no allocation and does not touch any
clock, so it stays outside the mandatory kernel clock
(xila-project/core#463) and runs on devices with 256 KB of RAM.

A zone is described by one POSIX TZ string, the `TZ` format of POSIX `tzset`:
`std offset [dst [offset] [,rule,rule]]`, for example
`CET-1CEST,M3.5.0,M10.5.0/3` for `Europe/Paris`. `TimeZone::parse` turns the
string into a `Copy` value of 32 bytes that owns its abbreviations: each
abbreviation is a fixed buffer of 7 bytes plus a length byte (the longest
abbreviation in tzdata 2026f is 5 characters, and POSIX requires at least 3),
and a name that does not fit is rejected with `ParseError::NameTooLong`. Each
of the two transition rules (`Mm.w.d[/time]`, `Jn[/time]`, `n[/time]`, with
signed times and hours above 24 as POSIX 2024 allows) is packed into one
32-bit `Rule` value: `size_of::<Rule>() == 4` bytes, and
`size_of::<TimeZone>() == 32` bytes on every target (asserted in a test).
"No daylight saving time" is encoded in the daylight saving offset field
itself, so the value stays 32 bytes with no extra tag.

## Defaults and limits of the parser

`TimeZone::parse` follows POSIX tzset; the defaults are:

- When the daylight saving rules are missing, the United States rules
  `M3.2.0/2,M11.1.0/2` apply.
- When the daylight saving offset is missing, it is one hour ahead of the
  standard offset.
- When a rule time is missing, it is 02:00:00.
- The zone offset is limited to 24 hours (POSIX allows 0 to 24), and rule
  times to 167 hours (POSIX 2024 allows -167 to 167).
- An unquoted zone name needs at least 3 ASCII letters; a quoted name
  `<...>` accepts only ASCII letters, digits, `+` and `-`.
- Minutes and seconds of an offset or a rule time are 0 to 59 (POSIX XBD
  8.3); 60 or more is rejected.
- A bare name with no offset, for example `UTC`, is rejected with
  `ParseError::MissingOffset`.

## Conversions

Conversions use plain `i64` seconds since the Unix epoch
(1970-01-01T00:00:00Z), not the `Timestamp` type of the `time` subsystem, so
that the library stays independent of the kernel clock. Convert a
`time::Timestamp` with `Timestamp::try_as_seconds`.

- UTC to local: `TimeZone::from_utc` returns the local date and time, the
  offset in seconds east of UTC, the zone abbreviation and whether daylight
  saving time applies.
- Local to UTC: `TimeZone::to_utc` returns an explicit `LocalToUtc` result:
  `Unique(instant)`, `Ambiguous { earlier, later }` for the repeated hour in
  autumn, or `Nonexistent` for the skipped hour in spring. A local time whose
  instants all fall outside the range that `i64` seconds can hold also
  returns `Nonexistent`. The input fields are not checked; use
  `DateTime::is_valid` to check them first.

```rust
use time_zone::{DateTime, LocalToUtc, TimeZone};

let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
let local = paris.from_utc(1_721_044_800); // 2024-07-15T12:00:00Z
assert_eq!(local.offset_seconds, 7200);
assert_eq!(local.abbreviation, "CEST");
assert!(local.daylight_saving);

// 2024-10-27 02:30:00 happens twice in Paris.
let result = paris.to_utc(DateTime {
    year: 2024, month: 10, day: 27, hour: 2, minute: 30, second: 0,
});
assert!(matches!(result, LocalToUtc::Ambiguous { .. }));
```

## IANA zone names

With a Cargo feature, a generated table maps IANA identifiers such as
`Europe/Paris` to the POSIX TZ string of the zone. The table is generated from
the IANA time zone database: the last line of every TZif v2+ file holds the
POSIX string. The names are sorted for binary search, and zones that share a
TZ string share it in one string blob; the lookup compares the name bytes
itself, without `memcmp`.

| Feature | Table |
| --- | --- |
| none (default) | no table |
| `iana-europe` | the Europe subset (87 zones) |
| `iana-all` | all zones (598 names, 94 unique TZ strings) |

The Europe subset holds every `Europe/` zone, the European zones outside that
prefix (`Africa/Ceuta`, `Arctic/Longyearbyen`, `Asia/Nicosia`,
`Atlantic/Azores`, `Atlantic/Canary`, `Atlantic/Faroe`, `Atlantic/Madeira`,
`Atlantic/Reykjavik`), and the backward links whose target is in the subset
(`Iceland`, `Portugal`, `Poland`, `GB`, and the others).

The table comes from tzdata **2026f** (`iana::TZDATA_VERSION`). The IANA time
zone database is released into the public domain by IANA
(<https://data.iana.org/time-zones/>); the generated files record that licence
and the provenance. The release tarball `tzdata2026f.tar.gz`
(<https://data.iana.org/time-zones/releases/tzdata2026f.tar.gz>) has SHA-256
`01b4a91cec5167ed48c11e8ab78f2048cead78ce94b0cafcad0ae8c5a3044d3d`.
Regenerate the table with `tools/generate_iana_table.py`, which reads the
version from the `version` file of the extracted release (and cross-checks it
against `tzdata.zi`) and fails if they disagree; see the script header for the
exact commands. Then run `cargo fmt`, and check the result with:

```
cargo test -p xila-time-zone --features iana-all
cargo test -p xila-time-zone --features iana-europe
```

## Code size

Measured with a probe that parses `CET-1CEST,M3.5.0,M10.5.0/3`, converts one
instant in both directions and walks the whole table, built with Rust 1.97,
`opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`:

| Configuration | thumbv7em-none-eabihf, linked (.text + .rodata) | wasm32-wasip1 cdylib (total / code / data) |
| --- | ---: | ---: |
| no table (parse and convert) | 7,094 bytes | 7,327 / 6,794 / 370 bytes |
| `iana-europe` (87 zones) | 9,607 bytes | 10,547 / 8,184 / 2,195 bytes |
| `iana-all` (598 zones) | 21,032 bytes | 21,953 / 8,185 / 13,600 bytes |

The name offsets of the table are stored as `u16`, because the name blob
(9,102 bytes for `iana-all`) fits; the generator uses `u32` only for a larger
blob. Against `u32` offsets, this saves 1,200 bytes with `iana-all` and 178
bytes with `iana-europe` on `thumbv7em-none-eabihf` (linked, measured) and the
same amount of data on `wasm32-wasip1`. The table rows above are the earlier
measurements minus these amounts, and the ESP32-S3 object sizes below are the
earlier ones minus the 1,198 and 176 bytes of offsets removed (2 bytes for each
of the 599 and 88 offsets).

BSS is 8 bytes in all Cortex-M configurations. For `xtensa-esp32s3-none-elf`
(the ESP32-S3 target of Platforms), the crate object inside the rlib measures
9,212 bytes with no table, 11,522 with `iana-europe` and 22,937 with
`iana-all` (code plus rodata, before the final link), built with
`RUSTFLAGS="-C opt-level=z" rustup run esp cargo build --release
-Zbuild-std=core --target xtensa-esp32s3-none-elf -p xila-time-zone` and the
matching `--features`. The linked size is smaller, because the linker removes
unused code; it is not reported, because the final link needs the
`xtensa-esp32s3-elf-gcc` linker of the esp-rs toolchain.

For comparison, the closed prototype #538 measured 1,963 bytes for UTC only,
18,300 bytes with Paris and 29,088 bytes with six zone identifiers on
Cortex-M, and 1,418 / 20,686 / 31,633 bytes on wasm: the POSIX TZ design
converts with about 7.1 KB of code and no table, and the full 598 zone table
costs about 21.0 KB on Cortex-M, less than the prototype's six zones.

Stack use on `thumbv7em-none-eabihf`, from `-Z emit-stack-sizes` (nightly):
the deepest frame of the crate is 128 bytes (`TimeZone::is_daylight_saving_at`,
called by both `from_utc` and `to_utc`, which are inlined into their caller),
and the deepest frame of the parse path is 64 bytes
(`Parser::parse_hours_minutes_seconds`); `TimeZone::parse` itself adds no
frame of its own.

## Limits

- A POSIX TZ string holds only the current rules of a zone, not its history.
  Instants far in the past or future can differ from the full IANA database by
  the rule changes in between.
- The current rules of some zones are not expressible in POSIX form, and their
  tzdata POSIX footer is an approximation: America/Havana (tzdata 2026f keeps
  -04 until 2027-11-07, while the footer string gives
  `CST5CDT,M3.2.0/0,M11.1.0/1`, so it is one hour off from 2026-11-01 to about
  2027-03), Asia/Gaza and Asia/Hebron, and Africa/Casablanca and
  Africa/El_Aaiun (Morocco during Ramadan, footer `<+00>0`).
- Rule times are stored with second precision inside the 32-bit `Rule`; zone
  offsets are limited to 24 hours and rule times to 167 hours, as POSIX 2024
  specifies.

## Tests

```
cargo test -p xila-time-zone
cargo test -p xila-time-zone --features iana-all
cargo test -p xila-time-zone --features iana-europe
```

The tests cover parsing of valid and invalid strings, conversions for
Europe/Paris, America/New_York, Australia/Sydney (southern hemisphere),
Asia/Kolkata (half hour offset, no daylight saving time), Europe/Dublin
(negative daylight saving offset), America/St_Johns (half hour daylight
saving shift), a zone offset of exactly 24:00:00, the next-year rule of the
southern hemisphere, the permanent daylight saving idiom
`EST5EDT,0/0,J365/25`, and rule times above 24 hours and negative, the exact
transition instants including the ambiguous and nonexistent local times, and
a comparison of a sample of instants with the host IANA database through the
`chrono` and `chrono-tz` development dependencies (used only in tests).
