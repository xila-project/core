#!/usr/bin/env python3
"""Real converter/Cargo/linker regression. Requires native binutils and npm ci.

All build products stay in Cargo target/OUT_DIR; evidence goes in --report.
Run from the repository root. Never cleans or removes a target directory.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


def snapshot(paths):
    return {
        str(path): {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                    "mtime_ns": path.stat().st_mtime_ns}
        for path in sorted(paths)
    }


def lvgl_outputs(messages):
    outputs = [Path(message["out_dir"]) for message in messages
               if message.get("reason") == "build-script-executed"
               and "lvgl_rust_sys" in message.get("package_id", "")]
    assert len(outputs) == 1, f"Expected one current LVGL build, got {outputs}"
    return [outputs[0] / "liblvgl.a", outputs[0] / "bindings.rs"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", type=Path, default=Path("target/font-pipeline"))
    parser.add_argument("--report", type=Path, default=Path("target/font-pipeline-evidence"))
    args = parser.parse_args()
    target, report = args.target.resolve(), args.report.resolve()
    report.mkdir(parents=True, exist_ok=True)
    records = []

    def command(cmd, name, env=None):
        start = time.monotonic()
        result = subprocess.run(cmd, env=env, text=True, capture_output=True)
        (report / f"{name}.stdout").write_text(result.stdout)
        (report / f"{name}.stderr").write_text(result.stderr)
        records.append({"command": cmd, "status": result.returncode,
                        "seconds": time.monotonic() - start})
        (report / "commands.json").write_text(json.dumps(records, indent=2))
        if result.returncode:
            raise RuntimeError(f"{name} failed; see {report / (name + '.stderr')}")
        return result.stdout

    results = []
    phases = [("en", "en"), ("fr", "en"), ("en", "en"), ("en", "fr")]
    # Additional unchanged-configuration invocations test Cargo's own freshness
    # tracking, not just the helper's direct cache validation.
    phases += [("en", "fr")] * 3 + [("en", "en")]
    for index, (locale, fallback) in enumerate(phases):
        name = f"{index}-{locale}-{fallback}"
        env = dict(os.environ, CARGO_TARGET_DIR=str(target),
                   EMBEDDED_I18N_LOCALE=locale, EMBEDDED_I18N_FALLBACK=fallback)
        output = command([
            "cargo", "rustc", "--manifest-path", "subsystems/graphics/tests/font_link/Cargo.toml",
            "--tests", "--message-format=json", "-vv", "--", "-C",
            f"link-arg=-Wl,-Map={report / 'link.map'}",
        ], name, env)
        messages = [json.loads(line) for line in output.splitlines() if line.startswith('{')]
        executables = [m["executable"] for m in messages if m.get("executable")]
        assert len(executables) == 1, executables
        executable = executables[0]
        command([executable, "--nocapture"], f"{name}-tests", env)
        out_dirs = [Path(m["out_dir"]) for m in messages
                    if m.get("reason") == "build-script-executed" and "graphics" in m.get("package_id", "")]
        assert len(out_dirs) == 1, out_dirs
        out = out_dirs[0]
        build_output = (out.parent / "output").read_text()
        current_sources = [Path(line.removeprefix("cargo:rerun-if-changed="))
                           for line in build_output.splitlines()
                           if line.startswith("cargo:rerun-if-changed=") and line.endswith(".generated.c")]
        generated = snapshot(current_sources)
        assert len(generated) == 24, f"Expected four reusable variants, got {len(generated)}"
        archive = out / "libfont_assets.a"
        members = command(["ar", "t", str(archive)], f"{name}-archive").splitlines()
        assert len(members) == 6 and all("font_" in member for member in members), members
        native = snapshot(lvgl_outputs(messages))
        nm = command(["nm", "-S", "--size-sort", executable], f"{name}-symbols")
        archive_nm = command(["nm", "-S", "--size-sort", str(archive)], f"{name}-archive-symbols")

        def bitmap_sizes(text):
            return sorted(int(match, 16) for match in re.findall(
                r"^[0-9a-f]+\s+([0-9a-f]+)\s+\w\s+glyph_bitmap$", text, re.M))

        sizes = bitmap_sizes(nm)
        assert len(sizes) == 6 and sizes == bitmap_sizes(archive_nm), sizes
        command(["size", "-A", executable], f"{name}-size")
        link_map = (report / "link.map").read_text()
        (report / f"{name}.map").write_text(link_map)
        for member in members:
            assert member in link_map, f"Missing selected object {member} from linker map"
        linked_fonts = set(re.findall(r"[\w-]+-font_\w+\.generated\.o", link_map))
        assert linked_fonts == set(members), (linked_fonts, members)
        stats = re.search(r"Font sources: generated (\d+), reused (\d+)", build_output)
        assert stats, "Missing converter reuse evidence"
        if index:
            assert native == results[0]["lvgl"], "Locale switch rebuilt LVGL or bindings"
        if 0 < index < 4 or index == 7:
            if index < 4:
                assert generated == results[0]["generated"], "Locale switch regenerated reusable C sources"
            assert stats.groups() == ("0", "24"), stats.groups()
        elif 4 <= index <= 6:
            assert stats.groups() == ("1", "23"), stats.groups()
            assert {p: v["sha256"] for p, v in generated.items()} == {
                p: v["sha256"] for p, v in results[0]["generated"].items()}
        results.append({"locale": locale, "fallback": fallback, "generated": generated,
                        "lvgl": native, "bitmap_sizes": sizes, "members": members,
                        "generation": stats.groups(), "out_dir": str(out)})
        (report / "results.json").write_text(json.dumps(results, indent=2))
        damaged = Path(next(iter(generated)))
        if index == 3:
            damaged.unlink()
        elif index == 4:
            Path(f"{damaged}.json").unlink()
            Path(f"{damaged}.partial").write_text("interrupted converter output")
        elif index == 5:
            damaged.write_text("corrupt cached output")
        elif index == 6:
            # A deliberately planted archive member must not survive rebuilding.
            stale = report / "stale-font.o"
            stale.write_bytes(subprocess.check_output(["ar", "p", str(archive), members[0]]))
            command(["ar", "r", str(archive), str(stale)], f"{name}-plant-stale")
            planted = command(["ar", "t", str(archive)], f"{name}-planted-members")
            assert "stale-font.o" in planted
    assert results[0]["bitmap_sizes"] == results[2]["bitmap_sizes"]
    assert results[1]["bitmap_sizes"] == results[3]["bitmap_sizes"]
    assert sum(results[0]["bitmap_sizes"]) < sum(results[1]["bitmap_sizes"])
    print(f"Verified selected-only linked fonts and reusable generation: {report}")


if __name__ == "__main__":
    main()
