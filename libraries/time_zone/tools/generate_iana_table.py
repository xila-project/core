#!/usr/bin/env python3
"""Generate the IANA zone name tables of the xila-time-zone crate.

The script reads the IANA time zone database and writes src/generated/all.rs
and src/generated/europe.rs next to this script. The last line of every TZif
v2+ file holds the POSIX TZ string of the zone; that string is what the table
maps the IANA name to.

Canonical input: an extracted tzdata release, for example the pinned release
tzdata 2026f:

    curl -sO https://data.iana.org/time-zones/releases/tzdata2026f.tar.gz
    # SHA-256: 01b4a91cec5167ed48c11e8ab78f2048cead78ce94b0cafcad0ae8c5a3044d3d
    tar xzf tzdata2026f.tar.gz
    python3 libraries/time_zone/tools/generate_iana_table.py --tzdata tzdata-2026f
    cargo fmt

The version is read from the `version` file of the release (and cross-checked
against the `# version` line of `tzdata.zi` when that file is present); the
script fails if they disagree. The script runs zic itself into a temporary
work directory (it honours TMPDIR) and removes that directory afterwards.

Alternatively a precompiled zoneinfo tree can be used, with an explicit
version (no default):

    python3 libraries/time_zone/tools/generate_iana_table.py \
        --zoneinfo /usr/share/zoneinfo --version 2026f

The Europe subset holds the `Europe/` zones, the European zones outside that
prefix listed in EUROPE_EXTRA_ZONES, and the backward links whose target is in
the subset (EUROPE_LINKS in --zoneinfo mode, computed from the `backward` file
in --tzdata mode).

The IANA time zone database is released into the public domain by IANA
(https://data.iana.org/time-zones/); the generated files record that
provenance. Run the iana tests afterwards to check that every generated string
parses:

    cargo test -p xila-time-zone --features iana-all
"""

import argparse
import pathlib
import re
import shutil
import subprocess
import sys
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

# Directories of a zoneinfo tree that repeat the same zones.
SKIP_DIRECTORIES = {"right", "posix"}

# European zones outside the Europe/ prefix.
EUROPE_EXTRA_ZONES = [
    "Africa/Ceuta",
    "Arctic/Longyearbyen",
    "Asia/Nicosia",
    "Atlantic/Azores",
    "Atlantic/Canary",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Iceland",
]

# Backward links whose target is a European zone (used in --zoneinfo mode,
# where the `backward` source file is not available).
EUROPE_LINKS = [
    "Arctic/Longyearbyen",
    "Asia/Istanbul",
    "Atlantic/Faeroe",
    "Atlantic/Jan_Mayen",
    "CET",
    "EET",
    "Eire",
    "Europe/Amsterdam",
    "Europe/Belfast",
    "Europe/Bratislava",
    "Europe/Busingen",
    "Europe/Copenhagen",
    "Europe/Guernsey",
    "Europe/Isle_of_Man",
    "Europe/Jersey",
    "Europe/Kiev",
    "Europe/Ljubljana",
    "Europe/Luxembourg",
    "Europe/Mariehamn",
    "Europe/Monaco",
    "Europe/Nicosia",
    "Europe/Oslo",
    "Europe/Podgorica",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Skopje",
    "Europe/Stockholm",
    "Europe/Tiraspol",
    "Europe/Uzhgorod",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Zagreb",
    "Europe/Zaporozhye",
    "GB",
    "GB-Eire",
    "Iceland",
    "MET",
    "Poland",
    "Portugal",
    "Turkey",
    "W-SU",
    "WET",
]

# A light check of the POSIX TZ grammar; the Rust tests parse every string
# strictly.
_NAME = r"(?:[A-Za-z]{3,}|<[^>]{3,}>)"
_HOURS_MINUTES_SECONDS = r"[+-]?\d{1,3}(?::\d{2}(?::\d{2})?)?"
_RULE = r"(?:M\d{1,2}\.\d\.\d|J\d{1,3}|\d{1,3})(?:/[+-]?\d{1,3}(?::\d{2}(?::\d{2})?)?)?"
LOOSE_TZ = re.compile(
    "^" + _NAME + _HOURS_MINUTES_SECONDS
    + "(?:" + _NAME + "(?:" + _HOURS_MINUTES_SECONDS + ")?(?:," + _RULE + "," + _RULE + ")?)?$"
)


def read_release_version(tzdata):
    """Read the version of a tzdata release and cross-check its sources."""
    version_file = tzdata / "version"
    if not version_file.is_file():
        raise SystemExit(f"no `version` file in {tzdata}; pass --zoneinfo and --version instead")
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


def europe_links_from_backward(tzdata):
    """Backward link names whose target (or `#=` comment) is a European zone."""
    def is_european(name):
        return name.startswith("Europe/") or name in EUROPE_EXTRA_ZONES

    links = []
    backward = tzdata / "backward"
    if not backward.is_file():
        return sorted(EUROPE_LINKS)
    for line in backward.read_text().splitlines():
        if not line.startswith("Link"):
            continue
        parts = line.split()
        target, name = parts[1], parts[2]
        comment = line.split("#=", 1)[1].strip() if "#=" in line else None
        if is_european(comment or target):
            links.append(name)
    return sorted(set(links))


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


def europe_subset(zones, links):
    """Names of the Europe subset."""
    return sorted(
        name
        for name in zones
        if name.startswith("Europe/") or name in EUROPE_EXTRA_ZONES or name in links
    )


def rust_string(text):
    """Escape one string as a Rust string literal body."""
    return text.replace("\\", "\\\\").replace('"', '\\"').replace("\0", "\\0")


def emit(path, version, zones, subset, command):
    """Write one generated table file."""
    names = sorted(zones) if subset is None else subset
    strings = sorted(set(zones[name] for name in names))
    string_index = {value: index for index, value in enumerate(strings)}

    names_blob = "".join(name + "\0" for name in names)
    name_offsets = []
    offset = 0
    for name in names:
        name_offsets.append(offset)
        offset += len(name) + 1
    name_offsets.append(offset)  # sentinel, the end of the blob

    strings_blob = "".join(value + "\0" for value in strings)
    string_offsets = []
    offset = 0
    for value in strings:
        string_offsets.append(offset)
        offset += len(value) + 1
    string_offsets.append(offset)  # sentinel

    zone_string = [string_index[zones[name]] for name in names]

    lines = [
        f"// Generated by {command}. Do not edit.",
        "//",
        "// Source: IANA time zone database (tzdata) " + version + ", public domain,",
        "// " + TZDATA_RELEASE_URL,
        "// SHA-256 of the release tarball: " + TZDATA_RELEASE_SHA256,
        "// The POSIX TZ string of each zone is the last line of its TZif v2+ file.",
        f"// Zones: {len(names)}. Unique TZ strings: {len(strings)}.",
        "",
        f'pub(crate) const TZDATA_VERSION: &str = "{version}";',
        "",
        "pub(crate) const NAMES: &str = concat!(",
    ]
    for name in names:
        lines.append(f'    "{rust_string(name)}\\0",')
    lines.append(");")
    lines.append("")
    lines.append("pub(crate) const NAME_OFFSETS: &[u32] = &[")
    for index in range(0, len(name_offsets), 8):
        lines.append("    " + ", ".join(str(value) for value in name_offsets[index : index + 8]) + ",")
    lines.append("];")
    lines.append("")
    lines.append("pub(crate) const ZONE_STRING: &[u16] = &[")
    for index in range(0, len(zone_string), 12):
        lines.append("    " + ", ".join(str(value) for value in zone_string[index : index + 12]) + ",")
    lines.append("];")
    lines.append("")
    lines.append("pub(crate) const STRINGS: &str = concat!(")
    for value in strings:
        lines.append(f'    "{rust_string(value)}\\0",')
    lines.append(");")
    lines.append("")
    lines.append("pub(crate) const STRING_OFFSETS: &[u32] = &[")
    for index in range(0, len(string_offsets), 8):
        lines.append("    " + ", ".join(str(value) for value in string_offsets[index : index + 8]) + ",")
    lines.append("];")
    lines.append("")
    path.write_text("\n".join(lines))
    return len(names), len(strings)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tzdata", type=pathlib.Path, default=None,
                        help="extracted tzdata release directory (canonical input; runs zic)")
    parser.add_argument("--zoneinfo", type=pathlib.Path, default=None,
                        help="directory of compiled TZif files (a zoneinfo tree)")
    parser.add_argument("--version", default=None,
                        help="tzdata version of a --zoneinfo input (required, no default)")
    parser.add_argument("--zic", default="zic", help="path of the zic compiler")
    parser.add_argument("--out-dir", type=pathlib.Path, default=None,
                        help="output directory, default: src/generated next to this script")
    args = parser.parse_args()

    out_dir = args.out_dir or pathlib.Path(__file__).resolve().parent.parent / "src" / "generated"
    out_dir.mkdir(parents=True, exist_ok=True)
    command = "tools/generate_iana_table.py"

    if args.tzdata is not None:
        if args.zoneinfo is not None:
            raise SystemExit("pass either --tzdata or --zoneinfo, not both")
        version = read_release_version(args.tzdata)
        workdir = pathlib.Path(tempfile.mkdtemp(prefix="xila-time-zone-generate-"))
        try:
            zoneinfo = compile_zoneinfo(args.tzdata, args.zic, workdir)
            zones = collect(zoneinfo)
            links = europe_links_from_backward(args.tzdata)
        finally:
            shutil.rmtree(workdir, ignore_errors=True)
    elif args.zoneinfo is not None:
        if args.version is None:
            raise SystemExit("--zoneinfo needs an explicit --version (there is no default)")
        version = args.version
        zones = collect(args.zoneinfo)
        links = sorted(EUROPE_LINKS)
    else:
        raise SystemExit("pass --tzdata or --zoneinfo")

    subset = europe_subset(zones, links)
    all_count = emit(out_dir / "all.rs", version, zones, None, command)
    europe_count = emit(out_dir / "europe.rs", version, zones, subset, command)
    print(f"all.rs: {all_count[0]} zones, {all_count[1]} unique TZ strings")
    print(f"europe.rs: {europe_count[0]} zones, {europe_count[1]} unique TZ strings")
    print(f"tzdata version recorded: {version}")


if __name__ == "__main__":
    main()
