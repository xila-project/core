#!/usr/bin/env python3
"""Generate the IANA zone name tables of the xila-time-zone crate.

The script reads one release of the IANA time zone database and writes one
table file per region into src/generated, plus src/generated/mod.rs. Each
table maps IANA names such as `Europe/Paris` to the POSIX TZ string of the
zone: the last line of its TZif v2+ file, as compiled by zic.

Canonical input: the pinned release tarball, whose SHA-256 the script checks
before it extracts the tarball:

    curl -sO https://data.iana.org/time-zones/releases/tzdata2026f.tar.gz
    python3 libraries/time_zone/tools/generate_iana_table.py --archive tzdata2026f.tar.gz
    cargo fmt

An extracted release directory is also accepted with `--tzdata DIR`, without
the checksum check. The version is read from the `version` file of the release
(and cross-checked against the `# version` line of `tzdata.zi` when that file
is present); the script fails if they disagree. The script runs zic itself
into a temporary work directory (it honours TMPDIR) and removes that directory
afterwards.

Classification of the names:

- A legacy alias is a name that tzdata defines only as a `Link` line of the
  `backward` file, for example `US/Eastern`, `Japan`, `GB` or `Asia/Calcutta`.
  Every other name is canonical, including the `Etc/` names and `GMT` of the
  `etcetera` file.
- The region of a name is its prefix before the first `/` when that prefix is
  one of REGIONS (`Europe/Paris` is in `europe`). A canonical name without such
  a prefix takes the region of the source file that defines it (SOURCE_REGION:
  `GMT` is in `etc`), or of BACKWARD_ZONE_REGION for the four zones that the
  `backward` file defines with `Zone` lines (`EST5EDT` is in `america`). A legacy alias without such a
  prefix takes the region of the zone that it names (its `#=` comment when
  there is one, otherwise its link target).
- `Factory` (the `factory` file, a placeholder for an unconfigured system) is
  in no table.

Each region file holds a table of the canonical names and a table of the legacy
aliases; the crate compiles the second one only with `iana-legacy-aliases`.

The IANA time zone database is released into the public domain by IANA
(https://data.iana.org/time-zones/); the generated files record that
provenance. Run the iana tests afterwards to check that every generated string
parses:

    cargo test -p xila-time-zone --features iana-all,iana-legacy-aliases
"""

import argparse
import hashlib
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

# Pinned tzdata release that the checked-in tables were generated from.
TZDATA_RELEASE_URL = "https://data.iana.org/time-zones/releases/tzdata2026f.tar.gz"
TZDATA_RELEASE_SHA256 = "01b4a91cec5167ed48c11e8ab78f2048cead78ce94b0cafcad0ae8c5a3044d3d"

# tzdata source files that zic compiles into the zone set (the same list as the
# default `make zones` target, without backzone).
ZONE_SOURCE_FILES = [
    "africa",
    "antarctica",
    "asia",
    "australasia",
    "backward",
    "etcetera",
    "europe",
    "northamerica",
    "southamerica",
    "factory",
]

# Regions: the Cargo feature is `iana-<region>`, the name prefix is the value.
REGIONS = {
    "africa": "Africa",
    "america": "America",
    "antarctica": "Antarctica",
    "arctic": "Arctic",
    "asia": "Asia",
    "atlantic": "Atlantic",
    "australia": "Australia",
    "etc": "Etc",
    "europe": "Europe",
    "indian": "Indian",
    "pacific": "Pacific",
}

# Region of a canonical name without a region prefix, by its source file.
SOURCE_REGION = {
    "africa": "africa",
    "antarctica": "antarctica",
    "asia": "asia",
    "australasia": "australia",
    "etcetera": "etc",
    "europe": "europe",
    "northamerica": "america",
    "southamerica": "america",
}

# Region of the canonical zones that the `backward` file defines with `Zone`
# lines (the backward-compatibility zones of the United States rules). They are
# not links, so they are canonical.
BACKWARD_ZONE_REGION = {
    "CST6CDT": "america",
    "EST5EDT": "america",
    "MST7MDT": "america",
    "PST8PDT": "america",
}

# Names in no table.
EXCLUDED_NAMES = {"Factory"}

# Directories of a zoneinfo tree that repeat the same zones.
SKIP_DIRECTORIES = {"right", "posix"}

# A light check of the POSIX TZ grammar; the Rust tests parse every string
# strictly.
_NAME = r"(?:[A-Za-z]{3,}|<[^>]{3,}>)"
_HOURS_MINUTES_SECONDS = r"[+-]?\d{1,3}(?::\d{2}(?::\d{2})?)?"
_RULE = r"(?:M\d{1,2}\.\d\.\d|J\d{1,3}|\d{1,3})(?:/[+-]?\d{1,3}(?::\d{2}(?::\d{2})?)?)?"
LOOSE_TZ = re.compile(
    "^" + _NAME + _HOURS_MINUTES_SECONDS
    + "(?:" + _NAME + "(?:" + _HOURS_MINUTES_SECONDS + ")?(?:," + _RULE + "," + _RULE + ")?)?$"
)


def extract_archive(archive, workdir):
    """Check the SHA-256 of the release tarball and extract it into workdir."""
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != TZDATA_RELEASE_SHA256:
        raise SystemExit(f"SHA-256 of {archive} is {digest}, expected {TZDATA_RELEASE_SHA256}")
    target = workdir / "tzdata"
    target.mkdir()
    with tarfile.open(archive) as release:
        release.extractall(target, filter="data")
    return target


def read_release_version(tzdata):
    """Read the version of a tzdata release and cross-check its sources."""
    version_file = tzdata / "version"
    if not version_file.is_file():
        raise SystemExit(f"no `version` file in {tzdata}")
    version = version_file.read_text().strip()
    zi = tzdata / "tzdata.zi"
    if zi.is_file():
        match = re.search(r"^#\s*version\s+(\S+)", zi.read_text(), re.MULTILINE)
        if match and match.group(1) != version:
            raise SystemExit(f"version mismatch: `version` says {version}, tzdata.zi says {match.group(1)}")
    return version


def compile_zoneinfo(tzdata, zic, workdir):
    """Run zic on the release sources into workdir; return the zoneinfo dir."""
    zoneinfo = workdir / "zoneinfo"
    sources = [tzdata / name for name in ZONE_SOURCE_FILES]
    missing = [str(path) for path in sources if not path.is_file()]
    if missing:
        raise SystemExit(f"missing tzdata source files: {', '.join(missing)}")
    subprocess.run([zic, "-d", str(zoneinfo), *map(str, sources)], check=True)
    return zoneinfo


def read_sources(tzdata):
    """The source file of every name, and the legacy aliases of `backward`:
    {alias: the zone that it names}."""
    sources = {}
    aliases = {}
    for file_name in ZONE_SOURCE_FILES:
        for line in (tzdata / file_name).read_text().splitlines():
            parts = line.split()
            if len(parts) >= 2 and parts[0] == "Zone":
                sources[parts[1]] = file_name
            elif len(parts) >= 3 and parts[0] == "Link":
                sources[parts[2]] = file_name
                if file_name == "backward":
                    comment = line.split("#=", 1)[1].strip() if "#=" in line else None
                    aliases[parts[2]] = comment or parts[1]
    if not aliases:
        raise SystemExit("no Link lines in the `backward` file")
    return sources, aliases


def posix_footer(path):
    """Return the POSIX TZ string at the end of a TZif v2+ file, or None."""
    data = path.read_bytes()
    if not data.startswith(b"TZif"):
        return None
    if data[4:5] == b"\x00":
        return None  # TZif v1 has no POSIX footer
    if not data.endswith(b"\n"):
        return None
    body = data[:-1]
    last_newline = body.rfind(b"\n")
    if last_newline < 0:
        return None
    footer = body[last_newline + 1 :].decode("ascii")
    return footer or None


def collect(zoneinfo):
    """Collect (name, TZ string) pairs from a zoneinfo tree."""
    zones = {}
    for path in sorted(zoneinfo.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(zoneinfo)
        if relative.parts[0] in SKIP_DIRECTORIES:
            continue
        name = relative.as_posix()
        footer = posix_footer(path)
        if footer is None:
            print(f"warning: no POSIX footer, skipped: {name}", file=sys.stderr)
            continue
        if not LOOSE_TZ.match(footer):
            print(f"warning: footer does not match the POSIX grammar: {name}: {footer}", file=sys.stderr)
            continue
        zones[name] = footer
    if not zones:
        raise SystemExit(f"no TZif v2+ files with a POSIX footer under {zoneinfo}")
    return zones


def prefix_region(name):
    """The region of a name from its prefix, or None."""
    if "/" not in name:
        return None
    prefix = name.split("/", 1)[0]
    for region, region_prefix in REGIONS.items():
        if prefix == region_prefix:
            return region
    return None


def classify(zones, sources, aliases):
    """{region: (canonical names, legacy names)}, each sorted."""
    def canonical_region(name):
        region = prefix_region(name)
        if region is not None:
            return region
        source = sources.get(name)
        if source == "backward" and name in BACKWARD_ZONE_REGION:
            return BACKWARD_ZONE_REGION[name]
        if source not in SOURCE_REGION:
            raise SystemExit(f"no region for {name} (source file {source})")
        return SOURCE_REGION[source]

    tables = {region: ([], []) for region in REGIONS}
    for name in sorted(zones):
        if name in EXCLUDED_NAMES:
            continue
        if name in aliases:
            target = aliases[name]
            while target in aliases:
                target = aliases[target]
            region = prefix_region(name) or canonical_region(target)
            tables[region][1].append(name)
        else:
            tables[canonical_region(name)][0].append(name)
    return tables


def rust_string(text):
    """Escape one string as a Rust string literal body."""
    return text.replace("\\", "\\\\").replace('"', '\\"').replace("\0", "\\0")


def table_lines(constant, zones, names, description):
    """The Rust lines of one table constant."""
    strings = sorted(set(zones[name] for name in names))
    string_index = {value: index for index, value in enumerate(strings)}
    if len(strings) > 0xFF:
        raise SystemExit(f"{constant}: {len(strings)} TZ strings do not fit the u8 index")

    def offsets(values):
        result = [0]
        for value in values:
            result.append(result[-1] + len(value) + 1)
        if result[-1] > 0xFFFF:
            raise SystemExit(f"{constant}: a blob of {result[-1]} bytes does not fit the u16 offsets")
        return result

    def numbers(values, per_line):
        return [
            "        " + ", ".join(str(value) for value in values[index : index + per_line]) + ","
            for index in range(0, len(values), per_line)
        ]

    def blob(field, values):
        # `concat!` only for two strings or more: Clippy rejects it otherwise.
        if len(values) < 2:
            return [f'    {field}: "' + "".join(rust_string(value) + "\\0" for value in values) + '",']
        return [f"    {field}: concat!("] + [f'        "{rust_string(value)}\\0",' for value in values] + ["    ),"]

    lines = [f"/// {description} (names: {len(names)}, TZ strings: {len(strings)})."]
    if constant == "LEGACY_ALIASES":
        lines.append('#[cfg(feature = "iana-legacy-aliases")]')
    lines.append(f"pub(crate) const {constant}: Table = Table {{")
    lines += blob("names", names)
    lines.append("    name_offsets: &[")
    lines += numbers(offsets(names), 10)
    lines.append("    ],")
    lines.append("    zone_string: &[")
    lines += numbers([string_index[zones[name]] for name in names], 16)
    lines.append("    ],")
    lines += blob("strings", strings)
    lines.append("    string_offsets: &[")
    lines += numbers(offsets(strings), 10)
    lines.append("    ],")
    lines.append("};")
    return lines


def emit_region(path, version, zones, region, canonical, legacy, command):
    """Write the table file of one region."""
    lines = [
        f"// Generated by {command}. Do not edit.",
        "//",
        "// Source: IANA time zone database (tzdata) " + version + ", public domain,",
        "// " + TZDATA_RELEASE_URL,
        "// SHA-256 of the release tarball: " + TZDATA_RELEASE_SHA256,
        "// The POSIX TZ string of each zone is the last line of its TZif v2+ file.",
        "",
        "use crate::iana::Table;",
        "",
    ]
    lines += table_lines("CANONICAL", zones, canonical, f"Canonical names of the {REGIONS[region]} region")
    lines.append("")
    lines += table_lines("LEGACY_ALIASES", zones, legacy, f"Legacy aliases of the {REGIONS[region]} region")
    lines.append("")
    path.write_text("\n".join(lines))


def emit_module(path, version, command):
    """Write mod.rs: one module per region feature, and the version."""
    lines = [
        f"// Generated by {command}. Do not edit.",
        "",
        "//! Generated IANA tables, one module per region.",
        "//!",
        "//! Run `tools/generate_iana_table.py` to regenerate them from the pinned",
        "//! IANA time zone database release, then run `cargo fmt`.",
        "",
        f'pub(crate) const TZDATA_VERSION: &str = "{version}";',
        "",
    ]
    for region in REGIONS:
        lines.append(f'#[cfg(feature = "iana-{region}")]')
        lines.append(f"pub(crate) mod {region};")
    lines.append("")
    path.write_text("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=pathlib.Path, default=None,
                        help="pinned release tarball (canonical input; checked against its SHA-256)")
    parser.add_argument("--tzdata", type=pathlib.Path, default=None,
                        help="extracted release directory (no checksum check)")
    parser.add_argument("--zic", default="zic", help="path of the zic compiler")
    parser.add_argument("--out-dir", type=pathlib.Path, default=None,
                        help="output directory, default: src/generated next to this script")
    args = parser.parse_args()
    if (args.archive is None) == (args.tzdata is None):
        raise SystemExit("pass either --archive or --tzdata")

    out_dir = args.out_dir or pathlib.Path(__file__).resolve().parent.parent / "src" / "generated"
    out_dir.mkdir(parents=True, exist_ok=True)
    command = "tools/generate_iana_table.py"

    workdir = pathlib.Path(tempfile.mkdtemp(prefix="xila-time-zone-generate-"))
    try:
        tzdata = extract_archive(args.archive, workdir) if args.archive else args.tzdata
        version = read_release_version(tzdata)
        sources, aliases = read_sources(tzdata)
        zones = collect(compile_zoneinfo(tzdata, args.zic, workdir))
    finally:
        shutil.rmtree(workdir, ignore_errors=True)

    missing = sorted(name for name in aliases if name not in zones)
    if missing:
        raise SystemExit(f"legacy aliases without a compiled zone: {', '.join(missing)}")

    tables = classify(zones, sources, aliases)
    for region, (canonical, legacy) in tables.items():
        emit_region(out_dir / f"{region}.rs", version, zones, region, canonical, legacy, command)
        print(f"{region}.rs: {len(canonical)} canonical names, {len(legacy)} legacy aliases")
    emit_module(out_dir / "mod.rs", version, command)
    total = sum(len(canonical) for canonical, _ in tables.values())
    legacy = sum(len(legacy) for _, legacy in tables.values())
    print(f"total: {total} canonical names, {legacy} legacy aliases; tzdata {version}")


if __name__ == "__main__":
    main()
