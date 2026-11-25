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

With Cargo features, generated tables map IANA identifiers such as
`Europe/Paris` to the POSIX TZ string of the zone. The tables are generated
from the IANA time zone database: the last line of every TZif v2+ file holds
the POSIX string. There is one table per region, and the region features
combine. Within a table the names are sorted for binary search, and zones that
share a TZ string share it in one string blob; the lookup compares the name
bytes itself, without `memcmp`.

| Feature | Names | Names by default | Legacy aliases |
| --- | --- | ---: | ---: |
| none (default) | no names (the `iana` module exists, with no names) | 0 | 0 |
| `iana-africa` | `Africa/...` | 52 | 5 |
| `iana-america` | `America/...`, `CST6CDT`, `EST5EDT`, `MST7MDT`, `PST8PDT` | 148 | 56 |
| `iana-antarctica` | `Antarctica/...` | 11 | 1 |
| `iana-arctic` | `Arctic/...` | 1 | 0 |
| `iana-asia` | `Asia/...` | 82 | 25 |
| `iana-atlantic` | `Atlantic/...` | 10 | 2 |
| `iana-australia` | `Australia/...` | 11 | 12 |
| `iana-etc` | `Etc/...` and `GMT`, the fixed offset zones | 29 | 15 |
| `iana-europe` | `Europe/...` | 58 | 17 |
| `iana-indian` | `Indian/...` | 11 | 0 |
| `iana-pacific` | `Pacific/...` | 38 | 13 |
| `iana-all` | every region | 451 | 146 |

- A region holds the names with its prefix only. The European zones with
  another prefix are in their own region: `Africa/Ceuta` in `iana-africa`,
  `Asia/Nicosia` in `iana-asia`, and `Atlantic/Azores`, `Atlantic/Canary`,
  `Atlantic/Faroe` and `Atlantic/Madeira` in `iana-atlantic`. A board for
  Europe that needs them enables these regions too.
- The four canonical names without a prefix (`EST5EDT` and the others) are
  zones that tzdata keeps for the United States rules; they are in
  `iana-america`. `Factory`, the placeholder for an unconfigured system, is in
  no table.
- The names by default are the canonical names (a `Zone` line, or a `Link`
  outside the `backward` file, such as `GMT` and the `Etc/` names) and the
  listed aliases: the 106 `backward` links that tzdata `zone.tab` lists for a
  country, for example `Europe/Amsterdam`, `Europe/Oslo`, `Europe/Stockholm`,
  `Atlantic/Reykjavik` and `Asia/Kuwait` (zones that tzdata merged into
  another one). So every country keeps the names that it uses.
- A legacy alias is any other `backward` link, for example `US/Eastern`,
  `Japan`, `GB` or `Asia/Calcutta`. `iana-legacy-aliases` adds them to every
  enabled region. An alias is in the region of its prefix, or, without a
  region prefix, in the region of the zone that it names (`US/Eastern` and
  `Japan` are in `iana-america` and `iana-asia`).
- An alias row holds its name and the index of the TZ string of the zone that
  it names, shared with that zone; no string is repeated within a region.
  Each region is sorted by name, aliases included.

The tables come from tzdata **2026f** (`iana::TZDATA_VERSION`). The IANA time
zone database is released into the public domain by IANA
(<https://data.iana.org/time-zones/>); the generated files record that licence
and the provenance. Regenerate the tables from the release tarball
`tzdata2026f.tar.gz`
(<https://data.iana.org/time-zones/releases/tzdata2026f.tar.gz>):

```
python3 libraries/time_zone/tools/generate_iana_table.py --archive tzdata2026f.tar.gz
cargo fmt
```

The script checks the SHA-256 of the tarball
(`01b4a91cec5167ed48c11e8ab78f2048cead78ce94b0cafcad0ae8c5a3044d3d`), reads the
version from the `version` file of the release (and cross-checks it against
`tzdata.zi`), runs zic and writes `src/generated`. The generated files must
not change when the script runs again on the same tarball.

## Code size

Measured with a probe that parses `CET-1CEST,M3.5.0,M10.5.0/3`, converts one
instant in both directions and walks every enabled table, linked for
`thumbv7em-none-eabihf` with Rust 1.97.0, `opt-level = "z"`, `lto = true`,
`codegen-units = 1`, `panic = "abort"`. The table cost is the linked size
(.text + .rodata) minus the 6,908 bytes of the probe with no table.

| Features | Linked size | Table cost | Table cost with `iana-legacy-aliases` |
| --- | ---: | ---: | ---: |
| none | 6,908 bytes | 0 | 0 |
| `iana-africa` | 8,344 bytes | 1,436 | 1,502 |
| `iana-america` | 10,878 bytes | 3,970 | 5,000 |
| `iana-antarctica` | 7,658 bytes | 750 | 774 |
| `iana-arctic` | 6,992 bytes | 84 | 84 |
| `iana-asia` | 8,982 bytes | 2,074 | 2,436 |
| `iana-atlantic` | 7,590 bytes | 682 | 752 |
| `iana-australia` | 7,644 bytes | 736 | 978 |
| `iana-etc` | 7,906 bytes | 998 | 1,166 |
| `iana-europe` | 8,496 bytes | 1,588 | 1,792 |
| `iana-indian` | 7,526 bytes | 618 | 618 |
| `iana-pacific` | 8,282 bytes | 1,374 | 1,566 |
| `iana-all` | 18,560 bytes | 11,652 | 14,008 |

Each cost above includes the lookup code (about 280 bytes), which several
regions share. A region repeats the TZ strings that it shares with another
region; `iana-all` costs 11,652 bytes by default (451 names) and 14,008 bytes
with the aliases (597 names). Offsets are 16-bit and string indexes 8-bit;
the generator fails if a table outgrows them. BSS is 8 bytes in all
configurations.

For `xtensa-esp32s3-none-elf` (the ESP32-S3 target of Platforms), the crate
object inside the rlib measures 9,325 bytes with no table, 11,033 with
`iana-europe`, 21,061 with `iana-all` and 23,415 with `iana-all` and
`iana-legacy-aliases` (text plus rodata, before the final link), built with
`RUSTFLAGS="-C opt-level=z" rustup run esp cargo build --release
-Zbuild-std=core --target xtensa-esp32s3-none-elf -p xila-time-zone` and the
matching `--features`. The linked size is smaller, because the linker removes
unused code.

For comparison, the closed prototype #538 measured 1,963 bytes for UTC only,
18,300 bytes with Paris and 29,088 bytes with six zone identifiers on
Cortex-M: the POSIX TZ design converts with about 6.9 KB of code and no table,
and the 451 names of all regions cost about 11.7 KB on Cortex-M.

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
cargo test -p xila-time-zone --features iana-europe
cargo test -p xila-time-zone --features iana-all
cargo test -p xila-time-zone --features iana-all,iana-legacy-aliases
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
