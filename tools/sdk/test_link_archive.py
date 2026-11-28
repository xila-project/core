"""Tests for `build.py link-archive`: the command that localizes one linked C program
into an archive whose only global defined symbol is its entry.

The command-construction tests run everywhere. The end-to-end test needs clang, ld.lld,
llvm-objcopy, llvm-ar and llvm-nm on the host and is skipped without them; it builds a
minimal SDK from the repository's own crt0.c and stubs.c and localizes two programs that
both define `main`, `malloc`, `helper` and a `static int counter`.
"""

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import build

ROOT = Path(build.ROOT)
SDK_SOURCE = Path(build.SDK_SOURCE)

# A stale LD_LIBRARY_PATH can break clang's driver lookup (`-print-prog-name`); the tools
# the test spawns do not need it.
os.environ.pop("LD_LIBRARY_PATH", None)


def have_tools(*names):
    for name in names:
        if shutil.which(name) is not None:
            continue
        result = subprocess.run(["clang", f"-print-prog-name={name}"],
                                capture_output=True, text=True)
        candidate = result.stdout.strip()
        if not (candidate and os.path.isabs(candidate) and os.path.exists(candidate)):
            return False
    return True


TOOLS = ("clang", "ld.lld", "llvm-objcopy", "llvm-ar", "llvm-nm", "llvm-readelf")


def tool(name):
    found = shutil.which(name)
    if found:
        return found
    result = subprocess.run(["clang", f"-print-prog-name={name}"],
                            capture_output=True, text=True, check=True)
    return result.stdout.strip()


def fake_sdk(directory):
    """An SDK directory layout with the four files link-archive requires."""
    lib = directory / "lib"
    lib.mkdir(parents=True)
    archives = {
        "crt0_linked": lib / "crt0_linked.o",
        "libxila_posix": lib / "libxila_posix.a",
        "libxila": lib / "libxila.a",
        "libc": lib / "libc.a",
    }
    for path in archives.values():
        path.write_bytes(b"")
    (directory / "sdk.json").write_text(json.dumps({"target": "x86_64-unknown-linux-gnu"}))
    return archives


class EntrySymbolTest(unittest.TestCase):
    def test_accepts_c_identifiers(self):
        for entry in ("x", "_", "_x9", "xila_linked_start", "MyApp_2", "A"):
            self.assertEqual(build.validate_entry_symbol(entry), entry)

    def test_rejects_non_identifiers(self):
        for entry in ("", "1abc", "9", "my-app", "my app", "a.b", "a b", "-x", "é", "a\nb"):
            with self.assertRaises(SystemExit, msg=entry):
                build.validate_entry_symbol(entry)


class NmArchiveSymbolsTest(unittest.TestCase):
    """Global defined symbol filtering: ignore debug labels (`N`) and locals, keep real globals."""

    def test_ignores_debug_labels_and_keeps_global_defined_symbols(self):
        sample = "\n".join([
            "libhexdump.a[xila_linked_hexdump.o]:",
            ".LASF0 N 0 ",
            ".LLRL18 N 0 ",
            ".LLST0 N 0 ",
            ".Ldebug_abbrev0 N 0 ",
            ".Ldebug_info0 N 0 ",
            ".Ldebug_line0 N 0 ",
            "main t 0 10",
            "counter d 0 4",
            "xila_linked_hexdump T 0 40",
            "stray_data D 40 4",
            "stray_weak W 44 8",
            "stray_rodata R 4c 4",
            "stray_bss B 50 4",
            "stray_abs A 0 ",
            "stray_weak_obj V 54 4",
            "stray_common C 4 4",
            "stray_ifunc i 58 4",
            "stray_unique u 5c 4",
        ])
        self.assertEqual(
            build.parse_nm_defined(sample),
            {
                "xila_linked_hexdump": "T",
                "stray_data": "D",
                "stray_weak": "W",
                "stray_rodata": "R",
                "stray_bss": "B",
                "stray_abs": "A",
                "stray_weak_obj": "V",
                "stray_common": "C",
                "stray_ifunc": "i",
                "stray_unique": "u",
            },
        )


class LinkArchivePlanTest(unittest.TestCase):
    """The command construction: combine, resolve weak undefined, rename, localize, pack."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xila-test-link-archive-")
        self.addCleanup(self.temporary.cleanup)
        self.work = Path(self.temporary.name) / "sdk"
        self.archives = fake_sdk(self.work)
        self.tools = {"lld": "ld.lld", "objcopy": "llvm-objcopy", "ar": "llvm-ar"}
        self.plan_dir = Path(self.temporary.name) / "plan"
        self.plan_dir.mkdir()

    def plan(self, entry="my_app", inputs=("app.o", "libapp.a"), builtins=(), output="out.a",
             weak_undefined=()):
        return build.link_archive_plan(
            self.archives, entry, Path(output), [Path(name) for name in inputs],
            [Path(name) for name in builtins], self.plan_dir, self.tools,
            weak_undefined=weak_undefined)

    def test_combines_crt0_inputs_and_sdk_archives_in_one_group(self):
        commands = self.plan()
        lld = commands[0]
        self.assertEqual(lld[0], "ld.lld")
        self.assertEqual(lld[1:4], ["-r", "-o", str(self.plan_dir / "combined.o")])
        self.assertIn("--undefined=xila_linked_start", lld)
        # crt0_linked.o first, then the caller's objects, then the group.
        self.assertEqual(lld[lld.index("--undefined=xila_linked_start") + 1],
                         str(self.archives["crt0_linked"]))
        start = lld.index("--start-group")
        end = lld.index("--end-group")
        self.assertEqual(lld[start + 1:end],
                         [str(self.archives["libxila_posix"]), str(self.archives["libxila"]),
                          str(self.archives["libc"])])
        inputs = lld[lld.index(str(self.archives["crt0_linked"])) + 1:start]
        self.assertEqual(inputs, ["app.o", "libapp.a"])

    def test_builtins_join_the_group(self):
        commands = self.plan(builtins=("libgcc.a",))
        lld = commands[0]
        self.assertEqual(lld[lld.index("--end-group") - 1], "libgcc.a")

    def test_resolves_every_weak_undefined_symbol_to_the_absolute_0(self):
        commands = self.plan(weak_undefined=("stderr", "xila_missing_hook"))
        resolve = commands[1]
        self.assertEqual(resolve[0:4], ["ld.lld", "-r", "-o", str(self.plan_dir / "resolved.o")])
        self.assertEqual(resolve[4:-1], ["--defsym=stderr=0", "--defsym=xila_missing_hook=0"])
        self.assertEqual(resolve[-1], str(self.plan_dir / "combined.o"))
        # Without weak undefined symbols the resolve step is a plain copy.
        self.assertEqual(self.plan()[1],
                         ["ld.lld", "-r", "-o", str(self.plan_dir / "resolved.o"),
                          str(self.plan_dir / "combined.o")])

    def test_renames_then_localizes_to_the_entry_symbol(self):
        commands = self.plan(entry="my_app")
        self.assertEqual(commands[2], [
            "llvm-objcopy", "--redefine-sym=xila_linked_start=my_app",
            str(self.plan_dir / "resolved.o"), str(self.plan_dir / "renamed.o")])
        self.assertEqual(commands[3], [
            "llvm-objcopy", "--keep-global-symbol=my_app",
            str(self.plan_dir / "renamed.o"), str(self.plan_dir / "my_app.o")])

    def test_packs_the_localized_object_into_the_output_archive(self):
        commands = self.plan(output="libmy_app.a")
        self.assertEqual(commands[4], ["llvm-ar", "rcs", "libmy_app.a",
                                       str(self.plan_dir / "my_app.o")])

    def test_plan_has_five_commands_in_order(self):
        self.assertEqual([command[0] for command in self.plan()],
                         ["ld.lld", "ld.lld", "llvm-objcopy", "llvm-objcopy", "llvm-ar"])


class LinkArchiveValidationTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xila-test-link-archive-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def args(self, **overrides):
        values = {"sdk": str(self.root / "sdk"), "entry": "my_app",
                  "output": str(self.root / "out.a"), "builtins": [], "allow_undefined": [],
                  "inputs": [str(self.root / "app.o")]}
        values.update(overrides)
        return types.SimpleNamespace(**values)

    def test_rejects_an_invalid_entry_symbol(self):
        fake_sdk(self.root / "sdk")
        with self.assertRaises(SystemExit):
            build.link_archive(self.args(entry="not-an-identifier"))

    def test_rejects_a_missing_sdk(self):
        with self.assertRaises(SystemExit) as raised:
            build.link_archive(self.args())
        self.assertIn("crt0_linked.o", str(raised.exception))

    def test_rejects_a_missing_input(self):
        fake_sdk(self.root / "sdk")
        with self.assertRaises(SystemExit) as raised:
            build.link_archive(self.args())
        self.assertIn("app.o", str(raised.exception))


class SdkBuiltinsTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xila-test-link-archive-")
        self.addCleanup(self.temporary.cleanup)
        self.sdk = Path(self.temporary.name)

    def test_host_target_has_no_builtins(self):
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "x86_64-unknown-linux-gnu"}))
        self.assertEqual(build.sdk_builtins(self.sdk, []), [])

    def test_extra_builtins_are_appended(self):
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "x86_64-unknown-linux-gnu"}))
        self.assertEqual(build.sdk_builtins(self.sdk, ["libgcc.a"]), [Path("libgcc.a")])

    def test_bare_metal_target_contributes_its_libgcc_only_when_installed(self):
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "thumbv7em-none-eabihf"}))
        builtins = build.sdk_builtins(self.sdk, [])
        for path in builtins:
            self.assertTrue(path.is_file(), f"{path} does not exist")
        # Without sdk.json (or an unknown target) only the explicit extras remain.
        self.assertEqual(build.sdk_builtins(self.sdk / "nowhere", ["x.a"]), [Path("x.a")])
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "not-a-target"}))
        self.assertEqual(build.sdk_builtins(self.sdk, []), [])

    def test_missing_cross_compiler_is_not_an_error(self):
        # A target whose cross GCC is not installed contributes no builtins, with a note
        # on stderr, instead of crashing with FileNotFoundError.
        missing = ["xila-test-definitely-not-a-compiler"]
        original = build.TARGETS
        build.TARGETS = {**original, "fake-missing-target": {"builtins": [*missing, "-print-libgcc-file-name"]}}
        self.addCleanup(setattr, build, "TARGETS", original)
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "fake-missing-target"}))
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(build.sdk_builtins(self.sdk, []), [])
        self.assertIn("not found", stderr.getvalue())
        self.assertIn("--builtins", stderr.getvalue())
        # The explicit extras still come through.
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(build.sdk_builtins(self.sdk, ["x.a"]), [Path("x.a")])

    def test_failing_cross_compiler_contributes_no_builtins(self):
        original = build.TARGETS
        build.TARGETS = {**original, "fake-failing-target": {"builtins": ["false"]}}
        self.addCleanup(setattr, build, "TARGETS", original)
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "fake-failing-target"}))
        self.assertEqual(build.sdk_builtins(self.sdk, []), [])


def nm_symbols(archive):
    """`llvm-nm` output as {name: type letter}: defined symbols have an address, undefined
    ones are `U`."""
    output = subprocess.run([tool("llvm-nm"), str(archive)],
                            capture_output=True, text=True, check=True).stdout
    symbols = {}
    for line in output.splitlines():
        if line.endswith(":") or not line.strip():
            continue
        parts = line.split()
        if len(parts) == 2:
            kind, name = parts
        elif len(parts) == 3:
            _address, kind, name = parts
        else:
            continue
        symbols[name] = kind
    return symbols


def nm_undefined(archive):
    """The undefined symbols of an archive as (weak, strong) name lists, posix format."""
    output = subprocess.run([tool("llvm-nm"), "--undefined-only", "--format=posix",
                             str(archive)], capture_output=True, text=True, check=True).stdout
    weak, strong = [], []
    for line in output.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            (strong if parts[1] == "U" else weak).append(parts[0])
    return weak, strong


class FakeSdkTestCase(unittest.TestCase):
    """A minimal SDK built from the repository's own crt0.c and stubs.c: the base of the
    end-to-end tests."""

    # The fake SDK's POSIX archive: a minimal xila_main that calls the program's main, as
    # the real posix/startup.c does.
    XILA_MAIN = """
extern int main(int argc, char **argv);
int xila_main(const void *startup) { (void)startup; return main(0, 0); }
"""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xila-test-link-archive-e2e-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.sdk = self.root / "sdk"
        self.build_fake_sdk()

    def compile(self, source, output, extra=()):
        run = [tool("clang"), "-c", str(source), "-o", str(output),
               "-std=gnu11", "-Os", "-ffunction-sections", "-fdata-sections",
               "-fno-stack-protector", "-fno-unwind-tables",
               "-fno-asynchronous-unwind-tables", *extra]
        subprocess.run(run, check=True, capture_output=True, text=True)

    def archive(self, output, members):
        subprocess.run([tool("llvm-ar"), "rcs", str(output), *[str(m) for m in members]],
                       check=True, capture_output=True, text=True)

    def build_fake_sdk(self):
        lib = self.sdk / "lib"
        lib.mkdir(parents=True)
        include = [f"-I{SDK_SOURCE / 'sdk'}", f"-I{SDK_SOURCE / 'include/xila'}",
                   f"-I{SDK_SOURCE / 'include'}"]
        # The real linked crt0 and stubs, compiled like the SDK compiles them: -fPIE, and
        # -DXILA_SDK_LINKED for crt0_linked.
        self.compile(SDK_SOURCE / "sdk/crt0.c", lib / "crt0_linked.o",
                     extra=["-fPIE", "-DXILA_SDK_LINKED", *include])
        self.compile(SDK_SOURCE / "sdk/stubs.c", self.root / "stubs.o", extra=["-fPIE", *include])
        posix = self.root / "posix_main.c"
        posix.write_text(self.XILA_MAIN)
        self.compile(posix, self.root / "posix_main.o", extra=["-fPIE"])
        self.archive(lib / "libxila.a", [self.root / "stubs.o"])
        self.archive(lib / "libxila_posix.a", [self.root / "posix_main.o"])
        self.archive(lib / "libc.a", [])
        (self.sdk / "sdk.json").write_text(json.dumps({"target": "x86_64-unknown-linux-gnu"}))

    def link(self, entry, output, inputs, allow_undefined=()):
        build.link_archive(types.SimpleNamespace(
            sdk=str(self.sdk), entry=entry, output=str(output), builtins=[],
            allow_undefined=list(allow_undefined), inputs=[str(p) for p in inputs]))


@unittest.skipUnless(have_tools(*TOOLS), "needs clang, ld.lld, llvm-objcopy, llvm-ar, llvm-nm")
class LinkArchiveEndToEndTest(FakeSdkTestCase):
    """Localize two real programs that define colliding symbols and check with nm that
    each archive exposes only its own entry as a global `T` symbol."""

    PROGRAM = """
#include <stddef.h>
static int counter = {base};
int helper(int x) {{ return x {op} 1; }}
void *malloc(size_t n) {{ (void)n; return &counter; }}
int main(int argc, char **argv) {{ (void)argc; (void)argv; return helper(counter); }}
"""

    def setUp(self):
        super().setUp()
        self.entries = {}
        for name, base, op in (("alpha", 100, "+"), ("beta", 200, "*")):
            self.build_program(name, base, op)

    def build_program(self, name, base, op):
        source = self.root / f"prog_{name}.c"
        source.write_text(self.PROGRAM.format(base=base, op=op))
        self.compile(source, self.root / f"prog_{name}.o", extra=["-fPIE"])
        entry = f"xila_linked_{name}_entry"
        output = self.root / f"lib{name}.a"
        self.link(entry, output, [self.root / f"prog_{name}.o"])
        self.entries[name] = (entry, output, base, op)

    def test_rebuilding_an_archive_replaces_the_old_entry(self):
        _, output, _, _ = self.entries["alpha"]
        entry, _, _, _ = self.entries["beta"]
        self.link(entry, output, [self.root / "prog_beta.o"])
        defined, _, _ = build.nm_archive_symbols(output, tool("llvm-nm"))
        self.assertEqual(defined, {entry: "T"})

    def test_rejects_lifecycle_sections_in_linked_programs(self):
        source = self.root / "lifecycle.c"
        source.write_text("""
__attribute__((constructor)) static void initialize(void) {}
int main(void) { return 0; }
""")
        program = self.root / "lifecycle.o"
        self.compile(source, program, extra=["-O0", "-fPIE"])
        with contextlib.redirect_stderr(io.StringIO()) as diagnostics:
            with self.assertRaises(SystemExit):
                self.link("xila_lifecycle_entry", self.root / "liblifecycle.a", [program])
        self.assertIn("linked programs cannot use lifecycle sections", diagnostics.getvalue())

    def test_each_archive_exposes_only_its_own_entry_as_a_global_t_symbol(self):
        for name, (entry, output, _base, _op) in self.entries.items():
            symbols = nm_symbols(output)
            global_defined = {symbol: kind for symbol, kind in symbols.items()
                              if kind.isupper() and kind != "U"}
            self.assertEqual(global_defined, {entry: "T"}, f"{name}: {symbols}")
            # The colliding program symbols are present but local.
            for colliding in ("main", "malloc", "helper", "counter"):
                self.assertIn(colliding, symbols, f"{name}: {colliding} missing")
                self.assertTrue(symbols[colliding].islower(),
                                f"{name}: {colliding} is global: {symbols[colliding]}")
            # crt0's globals are localized too; xila_main comes from the POSIX archive and
            # is localized like everything else (undefined only when the archive lacks it).
            for crt0_symbol in ("xila_sdk_api", "xila_sdk_process", "xila_sdk_fallback_api"):
                self.assertTrue(symbols[crt0_symbol].islower(), f"{name}: {crt0_symbol}")
            self.assertIn(symbols.get("xila_main", "U"), ("U", "t"), f"{name}: xila_main")
            # No weak undefined (`w`) symbol survives, and no strong undefined (`U`) one
            # either: nothing can bind to a host symbol such as glibc's `stderr`.
            weak, strong = nm_undefined(output)
            self.assertEqual(weak, [], f"{name}: weak undefined left: {weak}")
            self.assertEqual(strong, [], f"{name}: strong undefined left: {strong}")
            # Verify the build.py archive helper agrees with the direct checks above.
            self.assertEqual(
                build.nm_archive_symbols(output, tool("llvm-nm")),
                ({entry: "T"}, [], []),
            )

    def test_the_two_archives_link_into_one_binary_without_collisions(self):
        # A minimal host, as the firmware is one: it builds a startup block and calls both
        # entries through it. crt0 checks the startup and the API version before calling
        # xila_main, which calls the program's (localized) main.
        host = self.root / "host.c"
        host.write_text(
            '#include <stdio.h>\n#include "application.h"\n'
            + "".join(f"extern int {entry}(const XilaStartup *);\n"
                      for entry, _o, _b, _op in self.entries.values())
            + "int main(void) {\n"
              "  XilaApi api = {0};\n"
              "  api.size = sizeof(XilaApi);\n"
              "  api.version = (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR;\n"
              "  XilaStartup startup = {0};\n"
              "  startup.size = sizeof(XilaStartup);\n"
              "  startup.api = &api;\n"
            + "".join(f'  printf("{name}=%d\\n", {entry}(&startup));\n'
                      for name, (entry, _o, _b, _op) in self.entries.items())
            + "  return 0;\n}\n")
        binary = self.root / "host"
        include = [f"-I{SDK_SOURCE / 'include/xila'}", f"-I{SDK_SOURCE / 'include'}"]
        subprocess.run([tool("clang"), str(host), *include,
                        *[str(output) for _e, output, _b, _op in self.entries.values()],
                        "-o", str(binary)], check=True, capture_output=True, text=True)
        result = subprocess.run([str(binary)], capture_output=True, text=True, check=True)
        # helper(counter): alpha 100+1=101, beta 200*1=200 (op is applied to 1).
        self.assertEqual(result.stdout, "alpha=101\nbeta=200\n")


@unittest.skipUnless(have_tools(*TOOLS), "needs clang, ld.lld, llvm-objcopy, llvm-ar, llvm-nm")
class WeakUndefinedEndToEndTest(FakeSdkTestCase):
    """A program with weak undefined references, as the SDK's POSIX layer leaves them when
    an optional module is not linked, must see null for them and never a host symbol: the
    archive is linked into a default PIE binary that itself uses glibc's `stderr`."""

    WEAK_PROGRAM = """
#include <stddef.h>
extern void *stderr __attribute__((weak));
extern int xila_missing_hook(void) __attribute__((weak));
int main(void) { return (&stderr == NULL && xila_missing_hook == NULL) ? 0 : 1; }
"""

    def setUp(self):
        super().setUp()
        source = self.root / "prog_weak.c"
        source.write_text(self.WEAK_PROGRAM)
        self.compile(source, self.root / "prog_weak.o", extra=["-fPIE"])
        self.weak_archive = self.root / "libweak.a"
        self.link("xila_weak_entry", self.weak_archive, [self.root / "prog_weak.o"])

    def test_the_archive_keeps_no_weak_or_strong_undefined_symbol(self):
        weak, strong = nm_undefined(self.weak_archive)
        self.assertEqual(weak, [], f"weak undefined left: {weak}")
        self.assertEqual(strong, [], f"strong undefined left: {strong}")
        # The weak symbols are local absolute zeros now: the archive reads them as null.
        symbols = nm_symbols(self.weak_archive)
        self.assertEqual(symbols.get("stderr"), "a")
        self.assertEqual(symbols.get("xila_missing_hook"), "a")

    def test_weak_references_stay_null_in_a_pie_binary_that_links_glibc_stderr(self):
        host = self.root / "host.c"
        host.write_text(
            '#include <stdio.h>\n#include "application.h"\n'
            "extern int xila_weak_entry(const XilaStartup *);\n"
            "int main(void) {\n"
            "  XilaApi api = {0};\n"
            "  api.size = sizeof(XilaApi);\n"
            "  api.version = (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR;\n"
            "  XilaStartup startup = {0};\n"
            "  startup.size = sizeof(XilaStartup);\n"
            "  startup.api = &api;\n"
            '  fprintf(stderr, "host stderr ok\\n");\n'
            '  printf("weak_sees_null=%d\\n", xila_weak_entry(&startup) == 0);\n'
            "  return 0;\n}\n")
        binary = self.root / "host_pie"
        include = [f"-I{SDK_SOURCE / 'include/xila'}", f"-I{SDK_SOURCE / 'include'}"]
        # No -no-pie: the default link is a PIE, like a Rust test binary.
        subprocess.run([tool("clang"), str(host), *include, str(self.weak_archive),
                        "-o", str(binary)], check=True, capture_output=True, text=True)
        header = subprocess.run([tool("llvm-readelf"), "-h", str(binary)],
                                capture_output=True, text=True, check=True).stdout
        self.assertIn("DYN", header, "the host binary is not a PIE")
        result = subprocess.run([str(binary)], capture_output=True, text=True, check=True)
        # glibc's stderr works in the binary, and the archive's weak `stderr` saw null
        # instead of binding to it.
        self.assertEqual(result.stderr, "host stderr ok\n")
        self.assertEqual(result.stdout, "weak_sees_null=1\n")

    def test_a_strong_undefined_symbol_is_rejected_unless_allowed(self):
        source = self.root / "prog_strong.c"
        source.write_text("extern int missing_strong(void);\n"
                          "int main(void) { return missing_strong(); }\n")
        self.compile(source, self.root / "prog_strong.o", extra=["-fPIE"])
        with self.assertRaises(SystemExit) as raised:
            self.link("xila_strong_entry", self.root / "libstrong.a",
                      [self.root / "prog_strong.o"])
        self.assertIn("missing_strong", str(raised.exception))
        self.assertIn("--allow-undefined", str(raised.exception))
        # Allowed explicitly, the archive keeps exactly that one undefined symbol.
        self.link("xila_strong_entry", self.root / "libstrong.a",
                  [self.root / "prog_strong.o"], allow_undefined=["missing_strong"])
        weak, strong = nm_undefined(self.root / "libstrong.a")
        self.assertEqual(weak, [])
        self.assertEqual(strong, ["missing_strong"])

    def test_rejects_weak_undefined_function_with_direct_call_relocation(self):
        source = self.root / "prog_weak_call.c"
        source.write_text(
            "extern void missing_weak_fn(void) __attribute__((weak));\n"
            "int main(void) {\n"
            "  if (missing_weak_fn) missing_weak_fn();\n"
            "  return 0;\n"
            "}\n")
        self.compile(source, self.root / "prog_weak_call.o", extra=["-fPIE"])
        with self.assertRaises(SystemExit) as raised:
            self.link("xila_weak_call_entry", self.root / "libweakcall.a",
                      [self.root / "prog_weak_call.o"])
        self.assertIn("missing_weak_fn", str(raised.exception))
        self.assertIn("absolute 0", str(raised.exception))


def host_sdk_directory():
    """The built x86_64 host SDK under target/sdk, if `build.py build` produced one."""
    try:
        return build.sdk_directory(build.HOST_TARGET)
    except SystemExit:
        return None


@unittest.skipUnless(have_tools(*TOOLS) and sys.platform.startswith("linux"),
                     "needs clang, ld.lld, llvm-objcopy, llvm-ar, llvm-nm, llvm-readelf on Linux")
class RealSdkLinkedProgramTest(FakeSdkTestCase):
    """End to end against the real built host SDK: compile the POSIX C fixture with -fPIE,
    localize it with link-archive, link the archive into a default PIE host binary (the
    harness, which also uses glibc's stderr) and run the entry three times sequentially:
    a normal return, a run cancelled mid-pipe-read whose heap is then poisoned, and a
    normal return again. Skipped when no host SDK was built (run `build.py build` first)."""

    ENTRY = "my_app"

    def setUp(self):
        super().setUp()
        self.sdk = host_sdk_directory()
        if self.sdk is None:
            self.skipTest("no built host SDK under target/sdk: run `build.py build` first")

    def compile_sdk_object(self, source, output):
        """Compile like the SDK compiles its own objects: -fPIE, freestanding, SDK headers."""
        resource = subprocess.run(
            [tool("clang"), "--target=x86_64-unknown-none-elf", "-print-resource-dir"],
            capture_output=True, text=True, check=True).stdout.strip()
        include = SDK_SOURCE / "include"
        subprocess.run([tool("clang"), "--target=x86_64-unknown-none-elf", "-c", str(source),
                        "-o", str(output), "-std=gnu11", "-Os", "-fPIE",
                        "-ffunction-sections", "-fdata-sections", "-fno-stack-protector",
                        "-fno-unwind-tables", "-fno-asynchronous-unwind-tables",
                        "-D_GNU_SOURCE", "-nostdinc",
                        f"-isystem{self.sdk / 'include/posix'}",
                        f"-isystem{self.sdk / 'include'}",
                        f"-isystem{resource}/include",
                        f"-I{include / 'xila'}", f"-I{include}"],
                       check=True, capture_output=True, text=True)

    def test_linked_posix_program_survives_sequential_and_cancelled_runs(self):
        app = self.root / "app.c"
        app.write_bytes((Path(__file__).parent / "linked_e2e_app.c").read_bytes())
        app_object = self.root / "app.o"
        self.compile_sdk_object(app, app_object)
        archive = self.root / "libapp.a"
        self.link(self.ENTRY, archive, [app_object])
        # The archive exposes only its entry and leaves nothing undefined.
        symbols = nm_symbols(archive)
        global_defined = {s: k for s, k in symbols.items() if k.isupper() and k != "U"}
        self.assertEqual(global_defined, {self.ENTRY: "T"})
        weak, strong = nm_undefined(archive)
        self.assertEqual(weak, [])
        self.assertEqual(strong, [])

        # Link into a default PIE host binary (no -no-pie), like a Rust test binary; the
        # harness itself uses glibc's printf/fprintf, so a weak `stderr` left in the
        # archive would bind to glibc's and corrupt memory at exit.
        harness = self.root / "harness.c"
        harness.write_bytes((Path(__file__).parent / "linked_e2e_harness.c").read_bytes())
        binary = self.root / "harness"
        include = [f"-I{SDK_SOURCE / 'include/xila'}", f"-I{SDK_SOURCE / 'include'}"]
        subprocess.run([tool("clang"), str(harness), *include, str(archive),
                        "-o", str(binary), "-lpthread"],
                       check=True, capture_output=True, text=True)
        header = subprocess.run([tool("llvm-readelf"), "-h", str(binary)],
                                capture_output=True, text=True, check=True).stdout
        self.assertIn("DYN", header, "the host binary is not a PIE")
        result = subprocess.run([str(binary)], capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0,
                         f"the harness failed (a stale pointer into a freed run heap?):\n"
                         f"{result.stdout}\n{result.stderr}")
        self.assertIn("all runs clean", result.stderr)


if __name__ == "__main__":
    unittest.main()
