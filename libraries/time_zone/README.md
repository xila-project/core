# xila-time-zone

Time zone and daylight saving time conversion for Xila, based on POSIX TZ rule
strings. The crate is `no_std`, performs no allocation and does not touch any
clock, so it stays outside the mandatory kernel clock
(xila-project/core#463) and runs on devices with 256 KB of RAM.

A zone is described by one POSIX TZ string, the `TZ` format of POSIX `tzset`:
`std offset [dst [offset] [,rule,rule]]`, for example
`CET-1CEST,M3.5.0,M10.5.0/3` for `Europe/Paris`. `TimeZone::parse` turns the
string into a small `Copy` value with explicit errors for invalid strings. Each
of the two transition rules (`Mm.w.d[/time]`, `Jn[/time]`, `n[/time]`, with
signed times and hours above 24 as POSIX 2024 allows) is packed into one
32-bit `Rule` value: `size_of::<Rule>() == 4` bytes. The parsed `TimeZone`
borrows its two abbreviations from the input string and stores the offsets and
rules inline; it is `Copy` and about 24 bytes on a 32-bit target.

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
  autumn, or `Nonexistent` for the skipped hour in spring. It never picks one
  silently.

```rust
use time_zone::{LocalToUtc, TimeZone};

let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
let local = paris.from_utc(1_721_044_800); // 2024-07-15T12:00:00Z
assert_eq!(local.offset_seconds, 7200);
assert_eq!(local.abbreviation, "CEST");
assert!(local.dst);

// 2024-10-27 02:30:00 happens twice in Paris.
let date_time = local.date_time; // any DateTime works the same way
let result = paris.to_utc(time_zone::DateTime {
    year: 2024, month: 10, day: 27, hour: 2, minute: 30, second: 0,
});
assert!(matches!(result, LocalToUtc::Ambiguous { .. }));
```

## IANA zone names

With a Cargo feature, a generated table maps IANA identifiers such as
`Europe/Paris` to the POSIX TZ string of the zone. The table is generated from
the IANA time zone database: the last line of every TZif v2+ file holds the
POSIX string. The names are sorted for binary search, and zones that share a TZ
string share it in one string blob.

| Feature | Table |
| --- | --- |
| none (default) | no table |
| `iana-europe` | the Europe subset (64 zones) |
| `iana-all` | all zones (598 names, 94 unique TZ strings) |

The table comes from tzdata **2026f** (`iana::TZDATA_VERSION`). Regenerate it
with `tools/generate_iana_table.py` (see the script header for the exact
commands), then run `cargo fmt`, and check the result with:

```
cargo test -p xila-time-zone --features iana-all
cargo test -p xila-time-zone --features iana-europe
```

## Code size

Measured with a probe that parses `CET-1CEST,M3.5.0,M10.5.0/3`, converts one
instant in both directions and walks the whole table, built with Rust 1.97,
`opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`:

| Configuration | thumbv7em-none-eabihf (.text + .rodata) | wasm32-wasip1 module (total / code / data) |
| --- | ---: | ---: |
| no table (parse and convert) | 6,890 bytes | 7,309 / 7,025 / 113 bytes |
| `iana-europe` (64 zones) | 9,811 bytes | 10,259 / 8,395 / 1,687 bytes |
| `iana-all` (598 zones) | 22,684 bytes | 23,118 / 8,397 / 14,544 bytes |

BSS is 8 bytes in all Cortex-M configurations. For comparison, the closed
prototype #538 measured 1,963 bytes for UTC only, 18,300 bytes with Paris and
29,088 bytes with six zone identifiers on Cortex-M: the POSIX TZ design
converts with about 6.9 KB of code and no table, and the full 598 zone table
costs about 22.7 KB, less than the prototype's six zones.

## Limits

- A POSIX TZ string holds only the current rules of a zone, not its history.
  Instants far in the past or future can differ from the full IANA database by
  the rule changes in between.
- Zones whose future rules are not expressible in POSIX form use the
  approximation of the tzdata POSIX footer; for example Morocco during Ramadan.
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
saving shift) and rule times above 24 hours and negative, the exact transition
instants including the ambiguous and nonexistent local times, and a comparison
of a sample of instants with the host IANA database through the `chrono-tz`
development dependency.
