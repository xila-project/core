#!/usr/bin/env python3
"""Build the Xila native SDK for one target.

    uv run --no-project --with meson==X --with ninja==Y python tools/sdk/build.py build --target T

The SDK is picolibc (built from the pinned upstream release), the Xila POSIX layer,
crt0, the generated API stubs, the static-PIE linker script and a CMake toolchain
file, packed as target/sdk/xila-sdk-<abi>-<target>.tar.zst. Everything is produced
under target/sdk; nothing outside it is written. Python 3.12 or later, clang, lld,
llvm-ar, meson and ninja are required.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

ROOT = Path(__file__).resolve().parents[2]
SDK_SOURCE = ROOT / "libraries/abi/application"
LOADER = ROOT / "libraries/native_loader"
OUTPUT = ROOT / "target/sdk"

HOST_TARGET = "x86_64-unknown-linux-gnu"

# Per-target compiler configuration. Adding riscv32imac or Xtensa is a new entry (plus the
# loader-side checks in check_elf.py): the rest of the pipeline is target-independent.
TARGETS = {
    "x86_64-unknown-linux-gnu": {
        "triple": "x86_64-unknown-none-elf",
        "flags": [],
        "meson_cpu_family": "x86_64",
        "meson_cpu": "x86_64",
        "processor": "x86_64",
        "machine": "x86_64",
        # The Linux host test maps segments with mprotect: data must start on a page.
        "data_alignment": 4096,
        "builtins": None,
    },
    "thumbv7em-none-eabihf": {
        "triple": "thumbv7em-none-eabihf",
        "flags": ["-mcpu=cortex-m4", "-mfloat-abi=hard", "-mthumb"],
        "meson_cpu_family": "arm",
        "meson_cpu": "cortex-m4",
        "processor": "arm",
        "machine": "arm",
        "data_alignment": 16,
        # compiler-rt is not packaged for bare-metal Arm: take libgcc's 64-bit division and
        # soft double helpers from the Arm GNU toolchain until the SDK builds its own.
        "builtins": ["arm-none-eabi-gcc", "-mthumb", "-mcpu=cortex-m4", "-mfpu=fpv4-sp-d16",
                     "-mfloat-abi=hard", "-print-libgcc-file-name"],
    },
}

# Flags every SDK object is built with. The loader rejects constructors and TLS, so the
# build must never emit them; check_elf.py verifies the result.
COMMON_FLAGS = [
    "-std=gnu11", "-Os", "-fPIE", "-ffunction-sections", "-fdata-sections",
    "-fno-stack-protector", "-fno-unwind-tables", "-fno-asynchronous-unwind-tables",
    "-D_GNU_SOURCE", "-Wall", "-Wextra", "-Werror",
]

POSIX_SOURCES = ["errno", "exit_flush", "fd", "file", "heap", "onexit", "path", "startup", "status", "stderr", "stdin", "stdout", "time",
                 "unsupported"]


def pins():
    vars_ = tomllib.loads((ROOT / "mise.toml").read_text())["vars"]
    return {
        "version": vars_["picolibc_version"],
        "sha256": vars_["picolibc_sha256"],
    }


def run(command, **kwargs):
    print("+", " ".join(str(part) for part in command), flush=True)
    subprocess.run([str(part) for part in command], check=True, **kwargs)


def find_tool(name):
    """A tool on PATH, or next to clang (distributions install versioned LLVM directories)."""
    found = shutil.which(name)
    if found:
        return found
    result = subprocess.run(["clang", f"-print-prog-name={name}"], capture_output=True, text=True)
    candidate = result.stdout.strip()
    if candidate and os.path.isabs(candidate) and os.path.exists(candidate):
        return candidate
    sys.exit(f"error: {name} not found; install LLVM (clang, lld, llvm-ar)")


def fetch_picolibc(pin):
    """Download the pinned release and unpack it into its own directory. The archive is
    untrusted data until its SHA-256 matches the pin; nothing in it is executed except
    picolibc's own meson build."""
    version = pin["version"]
    source = OUTPUT / "src" / f"picolibc-{version}"
    stamp = source / ".xila-verified"
    if stamp.exists() and stamp.read_text().strip() == pin["sha256"]:
        return source
    download = OUTPUT / "downloads" / f"picolibc-{version}"
    download.mkdir(parents=True, exist_ok=True)
    archive = download / f"picolibc-{version}.tar.xz"
    if not archive.exists():
        url = f"https://github.com/picolibc/picolibc/releases/download/{version}/{archive.name}"
        print(f"downloading {url}", flush=True)
        urllib.request.urlretrieve(url, archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != pin["sha256"]:
        sys.exit(f"error: {archive} has sha256 {digest}, expected {pin['sha256']}")
    source.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as tar:
        tar.extractall(source.parent, filter="data")
    if not (source / "meson.build").exists():
        sys.exit(f"error: {archive} did not unpack to {source}")
    stamp.write_text(pin["sha256"] + "\n")
    return source


def meson_list(items):
    return "[" + ", ".join(f"'{item}'" for item in items) + "]"


def write_cross_file(target, path):
    config = TARGETS[target]
    compiler = ["clang", f"--target={config['triple']}", *config["flags"], "-nostdlib"]
    text = f"""[binaries]
c = {meson_list(compiler)}
c_ld = 'lld'
ar = '{find_tool("llvm-ar")}'
strip = '{find_tool("llvm-strip")}'
nm = '{find_tool("llvm-nm")}'

[host_machine]
system = 'none'
cpu_family = '{config["meson_cpu_family"]}'
cpu = '{config["meson_cpu"]}'
endian = 'little'

[built-in options]
c_args = {meson_list(["-fPIE", "-Os", "-ffunction-sections", "-fdata-sections",
                      "-Wno-unsupported-floating-point-opt"])}

[properties]
skip_sanity_check = true
needs_exe_wrapper = true
"""
    # Rewriting an identical file would make meson regenerate (and uv's throwaway
    # environment, which build.ninja remembers, is gone by the next run).
    if not path.exists() or path.read_text() != text:
        path.write_text(text)


# picolibc configuration. TLS is off and errno is a function so the loader never sees PT_TLS
# and no thread pointer is switched; the allocator and POSIX console are ours (the
# console's destructor would make the loader reject the image); integer printf is the
# default, with the float variants selectable by linking the other vfprintf.
PICOLIBC_OPTIONS = {
    "multilib": "false",
    "thread-local-storage": "false",
    "errno-function": "__xila_errno_location",
    "format-default": "integer",
    "semihost": "false",
    "picocrt": "false",
    "picocrt-lib": "false",
    "enable-malloc": "false",
    "posix-console": "false",
    "single-thread": "true",
    "stdio-exit-flush": "true",
    "tests": "false",
    "c_std": "c11",
    "debug": "false",
    "optimization": "s",
}


def build_picolibc(source, target, work):
    build = work / "picolibc-build"
    install = work / "picolibc"
    cross = work / "picolibc.cross"
    write_cross_file(target, cross)
    options = [f"-D{key}={value}" for key, value in PICOLIBC_OPTIONS.items()]
    if not (build / "build.ninja").exists():
        run(["meson", "setup", build, source, "--cross-file", cross, f"--prefix={install}",
             *options])
    else:
        # build.ninja remembers the meson that configured it, which may be gone.
        run(["meson", "setup", "--reconfigure", build, source, *options])
    run(["ninja", "-C", build])
    run(["meson", "install", "-C", build, "--quiet"])
    return install


# picolibc archive members the SDK removes from libc.a, by member name substring: (substring,
# reason). Each holds a constructor section the loader rejects, so an image that used any
# symbol of the member could not be loaded; a program that needs one then fails at link time,
# by name, instead.
#   bufio_exit_flush: registers the exit-time stdio flush with atexit() from a constructor,
#     and its flush unlinks every stream from the list as it goes, so a program that called
#     fflush(NULL) lost its later writes at exit. The POSIX layer defines _bufio_exit_flush
#     and __stdio_file_list itself (posix/exit_flush.c), with a flush that only walks the list.
#   exitprocs: picolibc's atexit table, run from a .fini_array_onexit entry that only a C
#     runtime which walks the fini array would call. The POSIX layer provides _on_exit and
#     runs the handlers from exit() itself (posix/onexit.c).
#   libc_ssp_stack_protector: seeds __stack_chk_guard from a constructor. Applications are
#     built with -fno-stack-protector.
#   interrupt.c: the x86_64 interrupt descriptor table setup (__idt), a constructor that is
#     only reachable from bare-metal x86 start-up code the SDK does not use.
DROPPED_MEMBERS = ("bufio_exit_flush", "exitprocs", "libc_ssp_stack_protector", "interrupt.c")


def drop_members(install):
    """Remove DROPPED_MEMBERS from the installed libc.a (llvm-ar d), when present: a member
    can be absent on a target (interrupt.c is x86 only) or already removed by an earlier run
    on the same installed copy. The constructor scan fails the build if anything else with a
    constructor remains, and the link fails if a second __stdio_file_list appears."""
    ar = find_tool("llvm-ar")
    archive = install / "lib/libc.a"
    members = subprocess.run([ar, "t", archive], capture_output=True, text=True, check=True)
    found = [m for m in members.stdout.split() if any(name in m for name in DROPPED_MEMBERS)]
    if found:
        run([ar, "d", archive, *dict.fromkeys(found)])
    if not any("bufio_exit_flush" in m for m in found):
        nm = subprocess.run([find_tool("llvm-nm"), "--defined-only", archive],
                            capture_output=True, text=True, check=True).stdout
        if "_bufio_exit_register" in nm:
            sys.exit("error: picolibc's exit-flush constructor is still in libc.a; "
                     "update DROPPED_MEMBERS")


# Sections the loader never runs. A constructor, destructor or init/fini hook in any
# member of any SDK archive would be linked into an image as soon as a symbol of that
# member is used, and the loader would reject the image (or, worse, silently skip it).
CONSTRUCTOR_SECTIONS = re.compile(
    r"^(\.rela?)?(\.(init|fini|preinit)_array\w*|\.ctors|\.dtors|\.init|\.fini)(\..*)?$")

# (archive, member, section) triples that are allowed to exist because the member can never
# be linked. Prefer deleting the member; anything listed here needs a reason.
CONSTRUCTOR_ALLOWLIST = {}


def scan_constructors(libraries):
    """Fail if any member of any archive (or crt0.o) holds a constructor section."""
    readelf = find_tool("llvm-readelf")
    offenders = []
    for path in sorted(libraries):
        output = subprocess.run([readelf, "--section-headers", "--wide", path],
                                capture_output=True, text=True, check=True).stdout
        member = path.name
        for line in output.splitlines():
            header = re.match(r"File: .*\((.+)\)$", line)
            if header:
                member = header.group(1)
                continue
            section = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+", line)
            if section and CONSTRUCTOR_SECTIONS.match(section.group(1)):
                key = (path.name, member, section.group(1))
                if key not in CONSTRUCTOR_ALLOWLIST:
                    offenders.append(key)
    if offenders:
        for archive, member, section in offenders:
            print(f"error: {archive}({member}) has {section}: the loader runs no constructors",
                  file=sys.stderr)
        sys.exit(1)
    print(f"constructor scan: {len(libraries)} files, no constructor, destructor or init section")


def compile_sdk(target, install, work):
    config = TARGETS[target]
    objects = work / "obj"
    objects.mkdir(exist_ok=True)
    include = [
        "-nostdinc",
        f"-isystem{install / 'include'}",
        f"-isystem{resource_dir(target)}/include",
        f"-isystem{SDK_SOURCE / 'sdk/posix/include'}",
        f"-I{SDK_SOURCE / 'include/xila'}",
        f"-I{SDK_SOURCE / 'sdk'}",
        f"-I{SDK_SOURCE / 'sdk/posix'}",
    ]
    base = ["clang", f"--target={config['triple']}", *config["flags"], *COMMON_FLAGS, *include]

    def compile_one(source, name):
        output = objects / f"{name}.o"
        run([*base, "-c", source, "-o", output])
        return output

    crt0 = compile_one(SDK_SOURCE / "sdk/crt0.c", "crt0")
    stubs = compile_one(SDK_SOURCE / "sdk/stubs.c", "stubs")
    posix = [compile_one(SDK_SOURCE / f"sdk/posix/{name}.c", name) for name in POSIX_SOURCES]
    ar = find_tool("llvm-ar")
    libraries = work / "lib"
    libraries.mkdir(exist_ok=True)
    for archive, members in (("libxila.a", [stubs]), ("libxila_posix.a", posix)):
        # A new file each time, renamed over the old one: updating in place would keep the
        # members of sources that no longer exist.
        staging = libraries / f"{archive}.{os.getpid()}"
        run([ar, "rcsD", staging, *members])
        os.replace(staging, libraries / archive)
    return crt0, libraries


def resource_dir(target):
    config = TARGETS[target]
    out = subprocess.run(["clang", f"--target={config['triple']}", "-print-resource-dir"],
                         capture_output=True, text=True, check=True)
    return out.stdout.strip()


def assemble(target, pin, install, crt0, libraries, work):
    header = (SDK_SOURCE / "include/xila/application.h").read_text()
    major = int(header.split("XILA_ABI_VERSION_MAJOR ")[1].split()[0])
    minor = int(header.split("XILA_ABI_VERSION_MINOR ")[1].split()[0])
    abi_version = f"{major}.{minor}"
    name = f"xila-sdk-{abi_version}-{target}"
    root = work / name
    # Files are overwritten in place: the build never deletes anything.
    (root / "include/xila").mkdir(parents=True, exist_ok=True)
    (root / "lib").mkdir(exist_ok=True)
    (root / "cmake").mkdir(exist_ok=True)
    (root / "share/xila/example/hello").mkdir(parents=True, exist_ok=True)

    shutil.copytree(install / "include", root / "include", dirs_exist_ok=True)
    shutil.copytree(SDK_SOURCE / "sdk/posix/include", root / "include", dirs_exist_ok=True)
    shutil.copy(SDK_SOURCE / "include/xila/application.h", root / "include/xila/")
    shutil.copy(SDK_SOURCE / "sdk/stubs.h", root / "include/xila/")
    shutil.copy(crt0, root / "lib/crt0.o")
    for archive in ("libxila.a", "libxila_posix.a"):
        shutil.copy(libraries / archive, root / "lib" / archive)
    shutil.copy(install / "lib/libc.a", root / "lib/libc.a")
    shutil.copy(install / "lib/libm.a", root / "lib/libm.a")  # empty: libm is inside libc.a
    shutil.copy(LOADER / "xila-static-pie.ld", root / "lib/xila-static-pie.ld")
    for example in (SDK_SOURCE / "sdk/example/hello").iterdir():
        shutil.copy(example, root / "share/xila/example/hello" / example.name)
    shutil.copy(SDK_SOURCE / "sdk/README.md", root / "README.md")

    config = TARGETS[target]
    template = (SDK_SOURCE / "sdk/cmake/xila-toolchain.cmake.in").read_text()
    replacements = {
        "@XILA_SDK_TARGET@": target,
        "@XILA_CLANG_TRIPLE@": config["triple"],
        "@XILA_CMAKE_PROCESSOR@": config["processor"],
        "@XILA_ARCH_FLAGS@": ";".join(config["flags"]),
        "@XILA_DATA_ALIGNMENT@": str(config["data_alignment"]),
        "@XILA_BUILTINS_COMMAND@": ";".join(config["builtins"] or []),
    }
    for key, value in replacements.items():
        template = template.replace(key, value)
    (root / "cmake/xila-toolchain.cmake").write_text(template)

    # Licences: picolibc's BSD terms travel with the library; Core's Apache-2.0 covers the rest.
    shutil.copy(pin_source(pin) / "COPYING.picolibc", root / "COPYING.picolibc")
    shutil.copy(ROOT / "license.md", root / "LICENSE-xila.md")

    clang = subprocess.run(["clang", "--version"], capture_output=True, text=True, check=True)
    (root / "sdk.json").write_text(json.dumps({
        "name": name,
        "target": target,
        "abi": abi_version,
        "picolibc": {"version": pin["version"], "sha256": pin["sha256"],
                     "options": PICOLIBC_OPTIONS},
        "compiler": clang.stdout.splitlines()[0],
        "data_alignment": config["data_alignment"],
        "entry": "_xila_start -> xila_main -> main",
    }, indent=2, sort_keys=True) + "\n")
    return name, root


def pin_source(pin):
    return OUTPUT / "src" / f"picolibc-{pin['version']}"


def pack(name, root):
    """A deterministic tar.zst: sorted names, no ownership, fixed timestamps."""
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    archive = OUTPUT / f"{name}.tar.zst"
    print(f"+ zstd -19 > {archive}", flush=True)
    with open(archive, "wb") as output:
        compressor = subprocess.Popen(["zstd", "-19", "-q", "-c"], stdin=subprocess.PIPE,
                                      stdout=output)
        with tarfile.open(fileobj=compressor.stdin, mode="w|", format=tarfile.PAX_FORMAT) as tar:
            for path in sorted([root, *root.rglob("*")],
                               key=lambda p: p.relative_to(root.parent).as_posix()):
                info = tar.gettarinfo(path, arcname=path.relative_to(root.parent).as_posix())
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mtime = epoch
                info.mode = 0o755 if path.is_dir() or os.access(path, os.X_OK) else 0o644
                if path.is_file():
                    with open(path, "rb") as handle:
                        tar.addfile(info, handle)
                else:
                    tar.addfile(info)
        compressor.stdin.close()
        if compressor.wait() != 0:
            sys.exit("error: zstd failed")
    return archive


def build(args):
    target = args.target
    pin = pins()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    source = fetch_picolibc(pin)
    work = OUTPUT / target
    work.mkdir(parents=True, exist_ok=True)
    install = build_picolibc(source, target, work)
    drop_members(install)
    crt0, libraries = compile_sdk(target, install, work)
    name, root = assemble(target, pin, install, crt0, libraries, work)
    scan_constructors([root / "lib/crt0.o", *sorted((root / "lib").glob("*.a"))])
    archive = pack(name, root)
    print(f"sdk directory: {root}")
    print(f"sdk archive:   {archive}")
    print(f"sha256:        {hashlib.sha256(archive.read_bytes()).hexdigest()}")


def map_sizes(text):
    """Bytes per origin and output section from an lld map: {origin: {section: size}}.
    Origins are the application object, crt0, and each SDK archive."""
    sizes = {}
    output_section = None
    for line in text.splitlines():
        match = re.match(r"\s+[0-9a-f]+\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s+(\S.*)$", line)
        if not match:
            continue
        size, rest = int(match.group(1), 16), match.group(2)
        member = re.match(r"(.+?):\((\.\S+)\)$", rest)
        if member is None:
            if rest.startswith("."):
                output_section = rest.split()[0]
            continue
        path = member.group(1)
        archive = re.match(r".*?([^/]+\.a)\(", path)
        if archive:
            origin = archive.group(1)
        elif path.endswith("crt0.o"):
            origin = "crt0.o"
        else:
            origin = "application"
        sizes.setdefault(origin, {}).setdefault(output_section, 0)
        sizes[origin][output_section] += size
    return sizes


def sdk_directory(target):
    found = sorted((OUTPUT / target).glob(f"xila-sdk-*-{target}"))
    if not found:
        sys.exit(f"error: no SDK for {target}: run the build command first")
    return found[-1]


def verify(args):
    """Build the hello example with the SDK's CMake toolchain, assert the ELF contract and
    report the image footprint."""
    from check_elf import check

    target = args.target
    sdk = sdk_directory(target)
    build = OUTPUT / target / "example-hello"
    run(["cmake", "-S", sdk / "share/xila/example/hello", "-B", build,
         f"-DCMAKE_TOOLCHAIN_FILE={sdk / 'cmake/xila-toolchain.cmake'}",
         "-DCMAKE_BUILD_TYPE=MinSizeRel", f"-DCMAKE_EXE_LINKER_FLAGS=-Map={build / 'hello.map'}"])
    run(["cmake", "--build", build])
    image = build / "hello.elf"
    problems = check(image.read_bytes(), TARGETS[target]["machine"])
    for problem in problems:
        print(f"{image}: {problem}", file=sys.stderr)
    if problems:
        sys.exit(1)
    print(f"{image}: ELF checks passed")
    run([find_tool("llvm-size"), image])
    print("\nBytes by origin (map):")
    sizes = map_sizes((build / "hello.map").read_text())
    print(f"{'origin':<18}{'.text':>8}{'.rodata':>9}{'.data+.got':>12}{'.bss':>7}")
    for origin, sections in sorted(sizes.items()):
        data = sections.get(".data", 0) + sections.get(".got", 0)
        print(f"{origin:<18}{sections.get('.text', 0):>8}{sections.get('.rodata', 0):>9}"
              f"{data:>12}{sections.get('.bss', 0):>7}")
    run([find_tool("llvm-size"), "-t", "libxila_posix.a", "libxila.a", "crt0.o"], cwd=sdk / "lib")


def main():
    # LLVM tools may live in a versioned directory that is not on PATH (clang finds them
    # next to itself): put lld's directory first so meson's and clang's linker probes work.
    lld = find_tool("ld.lld")
    os.environ["PATH"] = os.path.dirname(lld) + os.pathsep + os.environ["PATH"]
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build", help="build the SDK directory and archive")
    build_parser.add_argument("--target", default=HOST_TARGET, choices=sorted(TARGETS))
    build_parser.set_defaults(function=build)
    verify_parser = commands.add_parser("verify", help="build the example and check the ELF")
    verify_parser.add_argument("--target", default=HOST_TARGET, choices=sorted(TARGETS))
    verify_parser.set_defaults(function=verify)
    args = parser.parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
