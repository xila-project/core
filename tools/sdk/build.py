#!/usr/bin/env python3
"""Build the Xila native SDK for one target.

    uv run --no-project --with meson==X --with ninja==Y python tools/sdk/build.py build --target T
    python tools/sdk/build.py link-archive --sdk <sdk-dir> --entry my_app --output libmy_app.a app.o

The SDK is picolibc (built from the pinned upstream release), the Xila POSIX layer,
crt0, the generated API stubs, the static-PIE linker script and a CMake toolchain
file, packed as target/sdk/xila-sdk-<xila_version>-<target>.tar.zst. Everything is produced
under target/sdk; nothing outside it is written. Python 3.12 or later, clang, lld,
llvm-ar, meson and ninja are required.

`link-archive` turns one C program into a static archive whose only global defined
symbol is its entry: the firmware links the archive into itself and calls the entry
through `xila-executable`'s linked-program support, with every libc, POSIX, crt0 and
application symbol of the program localized, so two such programs never collide. Weak
undefined symbols are resolved to the absolute 0 first, so they read as null instead of
binding to a host symbol (glibc's `stderr` in a Linux test binary); any other undefined
symbol is an error unless the firmware provides it (`--allow-undefined`).
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
import tempfile
import tomllib
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

ROOT = Path(__file__).resolve().parents[2]
SDK_SOURCE = ROOT / "libraries/abi/application"
RUST_CRATE = SDK_SOURCE / "sdk/rust/xila-sys"
VERSION_CRATE = ROOT / "libraries/version"
LINKER_SCRIPT = RUST_CRATE / "xila-static-pie.ld"
SPLIT_LINKER_SCRIPT = RUST_CRATE / "xila-static-pie-split.ld"
RUST_CRATE_FILES = (
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    LINKER_SCRIPT.name,
    SPLIT_LINKER_SCRIPT.name,
)
OUTPUT = ROOT / "target/sdk"

HOST_TARGET = "x86_64-unknown-linux-gnu"

# Per-target compiler and linker configuration for the loader's static PIE contract.
TARGETS = {
    "x86_64-unknown-linux-gnu": {
        "triple": "x86_64-unknown-none-elf",
        "flags": [],
        "meson_cpu_family": "x86_64",
        "meson_cpu": "x86_64",
        "processor": "x86_64",
        "machine": "x86_64",
        "float_abi": None,
        # The Linux host test maps segments with mprotect: data must start on a page.
        "data_alignment": 4096,
        "builtins": None,
    },
    "aarch64-unknown-linux-gnu": {
        "triple": "aarch64-unknown-none-elf",
        "flags": [],
        "meson_cpu_family": "aarch64",
        "meson_cpu": "aarch64",
        "processor": "aarch64",
        "machine": "aarch64",
        "float_abi": None,
        # The Linux host maps segments with mprotect: data must start on a page.
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
        "float_abi": "hard",
        # compiler-rt is not packaged for bare-metal Arm: take libgcc's 64-bit division and
        # soft double helpers from the Arm GNU toolchain until the SDK builds its own.
        "builtins": ["arm-none-eabi-gcc", "-mthumb", "-mcpu=cortex-m4", "-mfpu=fpv4-sp-d16",
                     "-mfloat-abi=hard", "-print-libgcc-file-name"],
    },
    "riscv32imac-unknown-none-elf": {
        "triple": "riscv32-unknown-none-elf",
        # Call relaxation (pc-relative jal/c.jal) stays on: it is valid in a PIE and leaves
        # the R_RISCV_RELATIVE count of hello unchanged. Only gp relaxation would tie an
        # image to the firmware's global pointer; lld does it only with --relax-gp and a
        # __global_pointer$ symbol, neither of which the SDK has (check_elf.py rejects the
        # symbol).
        "flags": ["-march=rv32imac", "-mabi=ilp32"],
        "meson_cpu_family": "riscv32",
        "meson_cpu": "rv32imac",
        "processor": "riscv32",
        "machine": "riscv32",
        "data_alignment": 16,
        "float_abi": "soft",
        # Bare-metal GCC supplies the RV32 ILP32 multilib without needing newlib.
        "builtins": ["riscv64-unknown-elf-gcc", "-march=rv32imac", "-mabi=ilp32",
                     "-print-libgcc-file-name"],
        "builtins_fallbacks": [
            ["riscv32-esp-elf-gcc", "-march=rv32imac", "-mabi=ilp32",
             "-print-libgcc-file-name"],
        ],
    },
    "riscv32imafc-unknown-none-elf": {
        "triple": "riscv32-unknown-none-elf",
        "flags": ["-march=rv32imafc", "-mabi=ilp32f"],
        "meson_cpu_family": "riscv32",
        "meson_cpu": "rv32imafc",
        "processor": "riscv32",
        "machine": "riscv32",
        "data_alignment": 16,
        "float_abi": "single",
        # Bare-metal RISC-V GCC (or Espressif's riscv32-esp-elf-gcc) supplies the RV32
        # single-float multilib without needing newlib.
        "builtins": ["riscv64-unknown-elf-gcc", "-march=rv32imafc", "-mabi=ilp32f",
                     "-print-libgcc-file-name"],
        "builtins_fallbacks": [
            ["riscv32-esp-elf-gcc", "-march=rv32imafc", "-mabi=ilp32f",
             "-print-libgcc-file-name"],
        ],
    },
    "xtensa-esp32s3-elf": {
        "compiler": "xtensa-esp32s3-elf-gcc",
        "tool_prefix": "xtensa-esp32s3-elf-",
        "triple": None,
        # -mlongcalls keeps calls in range when a localized archive is linked into a
        # multi-megabyte firmware image. Static PIEs link with `-pie --no-dynamic-linker`
        # directly via GNU ld because Xtensa GCC ignores `-static-pie` (core#701).
        "flags": ["-mlongcalls"],
        "meson_cpu_family": "xtensa",
        "meson_cpu": "esp32s3",
        "processor": "xtensa",
        "machine": "xtensa",
        "data_alignment": 16,
        "float_abi": None,
        "split_rodata": True,
        "rust_example": False,
        "builtins": ["xtensa-esp32s3-elf-gcc", "-print-libgcc-file-name"],
    },
}

# Build the SDK archives without native TLS or lifecycle sections. The same
# archives serve loaded and linked applications.
COMMON_FLAGS = [
    "-std=gnu11", "-Os", "-fPIE", "-ffunction-sections", "-fdata-sections",
    "-fno-stack-protector", "-fno-unwind-tables", "-fno-asynchronous-unwind-tables",
    "-D_GNU_SOURCE", "-Wall", "-Wextra", "-Werror",
]

POSIX_SOURCES = ["environ", "environment", "errno", "exit_flush", "fd", "file", "heap", "onexit", "path", "process", "stack_protector", "startup", "status", "stderr", "stdin", "stdout",
                 "time", "poll", "unsupported", "thread", "thread_keys", "thread_sync", "libc_lock", "strtok", "random", "semaphore", "thread_rwlock",
                 "socket", "inet", "netdb"]


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
    if shutil.which("clang"):
        result = subprocess.run(["clang", f"-print-prog-name={name}"],
                                capture_output=True, text=True)
        candidate = result.stdout.strip()
        if candidate and os.path.isabs(candidate) and os.path.exists(candidate):
            return candidate
    sys.exit(f"error: {name} not found; install the required target toolchain")


LLVM_TOOLS = {
    "lld": "ld.lld",
    "ar": "llvm-ar",
    "strip": "llvm-strip",
    "nm": "llvm-nm",
    "objcopy": "llvm-objcopy",
    "readelf": "llvm-readelf",
    "readobj": "llvm-readobj",
    "size": "llvm-size",
}

GNU_TOOLS = {
    "lld": "ld",
    "ar": "ar",
    "strip": "strip",
    "nm": "nm",
    "objcopy": "objcopy",
    "readelf": "readelf",
    "readobj": "readelf",
    "size": "size",
}


def find_target_tool(target, role):
    """Find the target's binutils tool (LLVM for Clang targets, prefixed GNU binutils for GCC)."""
    prefix = TARGETS.get(target, {}).get("tool_prefix")
    if prefix:
        return find_tool(prefix + GNU_TOOLS[role])
    return find_tool(LLVM_TOOLS[role])


def compiler_command(target):
    """Compiler binary and target/architecture flags for `target`."""
    config = TARGETS[target]
    if config.get("compiler"):
        return [find_tool(config["compiler"]), *config["flags"]]
    return ["clang", f"--target={config['triple']}", *config["flags"]]


def compiler_include_dir(target):
    """The compiler's freestanding header directory (`stdarg.h`, `stddef.h`, `stdint.h`...)."""
    config = TARGETS[target]
    if config.get("compiler"):
        out = subprocess.run([find_tool(config["compiler"]), "-print-file-name=include"],
                             capture_output=True, text=True, check=True)
        return out.stdout.strip()
    return f"{resource_dir(target)}/include"


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
    is_gcc = bool(config.get("compiler"))
    compiler = [*compiler_command(target), "-nostdlib"]
    c_ld = "bfd" if is_gcc else "lld"
    c_args = ["-fPIE", "-Os", "-ffunction-sections", "-fdata-sections"]
    if not is_gcc:
        c_args.append("-Wno-unsupported-floating-point-opt")
    text = f"""[binaries]
c = {meson_list(compiler)}
c_ld = '{c_ld}'
ar = '{find_target_tool(target, "ar")}'
strip = '{find_target_tool(target, "strip")}'
nm = '{find_target_tool(target, "nm")}'

[host_machine]
system = 'none'
cpu_family = '{config["meson_cpu_family"]}'
cpu = '{config["meson_cpu"]}'
endian = 'little'

[built-in options]
c_args = {meson_list(c_args)}

[properties]
skip_sanity_check = true
needs_exe_wrapper = true
"""
    # Rewriting an identical file would make meson regenerate (and uv's throwaway
    # environment, which build.ninja remembers, is gone by the next run). Report a change to
    # an existing file: meson keeps the old compiler flags across --reconfigure.
    changed = path.exists() and path.read_text() != text
    if not path.exists() or changed:
        path.write_text(text)
    return changed


# picolibc configuration. TLS is off and errno is a function so the loader never sees PT_TLS
# and no thread pointer is switched; the allocator and POSIX console are ours (the
# console has a destructor, which a linked application cannot contain); integer printf is the
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
    "single-thread": "false",
    "stdio-locking": "true",
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
    cross_changed = write_cross_file(target, cross)
    options = [f"-D{key}={value}" for key, value in PICOLIBC_OPTIONS.items()]
    if not (build / "build.ninja").exists():
        run(["meson", "setup", build, source, "--cross-file", cross, f"--prefix={install}",
             *options])
    elif cross_changed:
        # --reconfigure keeps the compiler and flags of the original cross file, so a changed
        # cross file (a new -march or -m flag) would silently not apply. --wipe empties the
        # build directory and configures it again from the cross file as it is now.
        run(["meson", "setup", "--wipe", build, source, "--cross-file", cross,
             f"--prefix={install}", *options])
    else:
        # build.ninja remembers the meson that configured it, which may be gone.
        run(["meson", "setup", "--reconfigure", build, source, *options])
    run(["ninja", "-C", build])
    run(["meson", "install", "-C", build, "--quiet"])
    return install


# picolibc archive members the SDK removes from libc.a, by exact member name: (member,
# targets, reason); targets is None for every target. Each holds a constructor section the
# loader rejects or fallback implementations the SDK replaces.
#   libc_stdio_bufio_exit_flush.c.o: registers the exit-time stdio flush with atexit() from
#     a constructor, and its flush unlinks every stream from the list as it goes, so a
#     program that called fflush(NULL) lost its later writes at exit. The POSIX layer
#     defines _bufio_exit_flush and __stdio_file_list itself (posix/exit_flush.c), with a
#     flush that only walks the list.
#   libc_stdlib_exitprocs.c.o: picolibc's atexit table, run from a .fini_array_onexit entry
#     that only a C runtime which walks the fini array would call. The POSIX layer provides
#     _on_exit and runs the handlers from exit() itself (posix/onexit.c).
#   libc_ssp_stack_protector.c.o: seeds __stack_chk_guard from a constructor. The POSIX
#     layer defines __stack_chk_guard and __stack_chk_fail (posix/stack_protector.c) and
#     seeds the guard from xila_posix_initialize.
#   interrupt.c.o (x86_64 and aarch64): bare-metal interrupt/vector-table setup placed in
#     `.init`, reachable only from bare-metal start-up code the SDK does not use. Arm's
#     member of the same name (the vector table and default handlers) has no constructor
#     and stays.
# The POSIX layer provides the retarget locks and the nonblocking ftrylockfile.
# It also provides getenv, setenv and unsetenv over the ABI. Remove picolibc's
# private environment and its empty environ, which cannot represent the ABI's state.
# Picolibc's putenv copies the assignment and calls the SDK's setenv.
# Use the process capability instead of picolibc's fallback getpid.
DROPPED_MEMBERS = (
    ("libc_misc_lock.c.o", None),
    ("libc_stdio_ftrylockfile.c.o", None),
    ("libc_stdio_bufio_exit_flush.c.o", None),
    ("libc_stdlib_exitprocs.c.o", None),
    ("libc_stdlib__findenv.c.o", None),
    ("libc_stdlib_environ.c.o", None),
    ("libc_stdlib_getenv.c.o", None),
    ("libc_stdlib_setenv.c.o", None),
    ("libc_stdlib_unsetenv.c.o", None),
    ("libos_fallback_getpid.c.o", None),
    ("libc_ssp_stack_protector.c.o", None),
    ("interrupt.c.o", ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")),
)


def drop_members(install, target):
    """Remove DROPPED_MEMBERS from the installed libc.a (ar d), when present: a member
    can be absent on a target or already removed by an earlier run on the same installed
    copy. The constructor scan fails the build if anything else with a constructor remains,
    and the link fails if a second __stdio_file_list appears."""
    ar = find_target_tool(target, "ar")
    archive = install / "lib/libc.a"
    members = subprocess.run([ar, "t", archive], capture_output=True, text=True, check=True)
    names = [name for name, targets in DROPPED_MEMBERS if targets is None or target in targets]
    found = [m for m in dict.fromkeys(members.stdout.split()) if m in names]
    if found:
        run([ar, "d", archive, *found])
    if "libc_stdio_bufio_exit_flush.c.o" not in found:
        nm = subprocess.run([find_target_tool(target, "nm"), "--defined-only", archive],
                            capture_output=True, text=True, check=True).stdout
        if "_bufio_exit_register" in nm:
            sys.exit("error: picolibc's exit-flush constructor is still in libc.a; "
                     "update DROPPED_MEMBERS")


# The firmware link could run these sections outside the linked program's launch.
CONSTRUCTOR_SECTIONS = re.compile(
    r"^(\.rela?)?(\.(init|fini|preinit)_array\w*|\.ctors|\.dtors|\.init|\.fini)(\..*)?$")

# (archive, member, section) triples that are allowed to exist because the member can never
# be linked. Prefer deleting the member; anything listed here needs a reason.
CONSTRUCTOR_ALLOWLIST = {}


def scan_constructors(libraries, target=HOST_TARGET, *, linked=False):
    """Fail if any member of any archive (or crt0.o) holds a constructor section."""
    readelf = find_target_tool(target, "readelf")
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
            section = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+(\S+)", line)
            if section and (CONSTRUCTOR_SECTIONS.match(section[1])
                            or section[2] in ("INIT_ARRAY", "FINI_ARRAY", "PREINIT_ARRAY")):
                key = (path.name, member, section.group(1))
                if key not in CONSTRUCTOR_ALLOWLIST:
                    offenders.append(key)
    if offenders:
        reason = ("linked programs cannot use lifecycle sections" if linked
                  else "SDK archives must support linked programs")
        for archive, member, section in offenders:
            print(f"error: {archive}({member}) has {section}: {reason}",
                  file=sys.stderr)
        sys.exit(1)
    print(f"constructor scan: {len(libraries)} files, no constructor, destructor or init section")


def compile_sdk(target, install, work):
    objects = work / "obj"
    objects.mkdir(exist_ok=True)
    include = [
        "-nostdinc",
        f"-isystem{SDK_SOURCE / 'sdk/posix/include/posix'}",
        f"-isystem{SDK_SOURCE / 'sdk/posix/include'}",
        f"-isystem{install / 'include'}",
        f"-isystem{compiler_include_dir(target)}",
        f"-I{SDK_SOURCE / 'include/xila'}",
        f"-I{SDK_SOURCE / 'include'}",
        f"-I{SDK_SOURCE / 'sdk'}",
        f"-I{SDK_SOURCE / 'sdk/posix'}",
    ]
    base = [*compiler_command(target), *COMMON_FLAGS, *include]

    def compile_one(source, name, defines=()):
        output = objects / f"{name}.o"
        run([*base, *defines, "-c", source, "-o", output])
        return output

    crt0 = compile_one(SDK_SOURCE / "sdk/crt0.c", "crt0")
    # The linked variant of crt0: no ABI note, no _xila_start, xila_linked_start instead.
    # It keeps -fPIE like every other object: position-independent code links into both
    # the firmware's non-PIE image and PIE host binaries (Linux tests, Rust test
    # binaries), which non-PIC code cannot (R_X86_64_32S in a PIE link).
    crt0_linked = compile_one(SDK_SOURCE / "sdk/crt0.c", "crt0_linked", ["-DXILA_SDK_LINKED"])
    stubs = compile_one(SDK_SOURCE / "sdk/stubs.c", "stubs")
    graphics = compile_one(SDK_SOURCE / "sdk/graphics.c", "graphics")
    posix = [compile_one(SDK_SOURCE / f"sdk/posix/{name}.c", name) for name in POSIX_SOURCES]
    ar = find_target_tool(target, "ar")
    libraries = work / "lib"
    libraries.mkdir(exist_ok=True)
    for archive, members in (("libxila.a", [stubs, graphics]), ("libxila_posix.a", posix)):
        # A new file each time, renamed over the old one: updating in place would keep the
        # members of sources that no longer exist.
        staging = libraries / f"{archive}.{os.getpid()}"
        run([ar, "rcsD", staging, *members])
        os.replace(staging, libraries / archive)
    return crt0, crt0_linked, libraries


# The entry symbol of a link-archive program: a valid C identifier.
ENTRY_SYMBOL = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")

# Symbols the firmware itself defines, so a localized archive may leave them undefined:
# `__xila_current_startup` backs xila-sys's linked runtime. Callers can allow more with
# --allow-undefined.
FIRMWARE_DEFINED = frozenset({"__xila_current_startup"})


def validate_entry_symbol(entry):
    """A C identifier: the firmware references the entry by this exact symbol name."""
    if not ENTRY_SYMBOL.match(entry):
        sys.exit(f"error: --entry {entry!r} is not a C identifier "
                 "(expected ^[A-Za-z_][A-Za-z0-9_]*$)")
    return entry


def undefined_symbols(obj, nm):
    """The undefined symbols of a relocatable object as (weak, strong) name lists.

    `llvm-nm --undefined-only --format=posix` prints one `name kind value size` line per
    undefined symbol: kind `U` is a strong undefined reference, `w`/`v` a weak one, which
    a host link would satisfy with any matching symbol (glibc's `stderr` in a Linux test
    binary) instead of leaving it null."""
    output = subprocess.run([nm, "--undefined-only", "--format=posix", str(obj)],
                            capture_output=True, text=True, check=True).stdout
    weak, strong = [], []
    for line in output.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        (strong if parts[1] == "U" else weak).append(parts[0])
    return weak, strong


# Relocation types that cannot target an absolute-0 symbol in a PIE link (direct
# PC-relative or PLT calls). Data references through a GOT (`*GOT*`) are safe.
ZERO_UNSAFE_RELOC = re.compile(r"(?:PLT|PC32|PC24|CALL|JUMP|BRANCH)")
RELOC_LINE = re.compile(r"^\s*0x[0-9a-fA-F]+\s+(\S+)\s+(\S+)")
READELF_RELOC_LINE = re.compile(r"^\s*[0-9a-fA-F]+\s+[0-9a-fA-F]+\s+(\S+)\s+[0-9a-fA-F]+\s+(\S+)")


def unsafe_zero_relocations(obj, weak_names, readobj):
    """Return sorted `(symbol, relocation)` pairs where a weak undefined symbol that
    `link-archive` is about to resolve to absolute 0 is referenced by a direct call or
    PC-relative relocation. On x86_64 PIE links, lld and GNU ld reject such relocations
    against absolute symbols; the SDK must provide a weak hidden default instead."""
    if not weak_names:
        return []
    weak_set = set(weak_names)
    use_readelf = Path(readobj).name.endswith("readelf")
    command = [readobj, "--relocs", "--wide", str(obj)] if use_readelf else [readobj, "--relocs", str(obj)]
    pattern = READELF_RELOC_LINE if use_readelf else RELOC_LINE
    output = subprocess.run(command, capture_output=True, text=True, check=True).stdout
    bad = set()
    for line in output.splitlines():
        match = pattern.match(line)
        if not match:
            continue
        reloc, symbol = match.group(1), match.group(2)
        if symbol not in weak_set or "GOT" in reloc:
            continue
        if ZERO_UNSAFE_RELOC.search(reloc):
            bad.add((symbol, reloc))
    return sorted(bad)


def sdk_libraries(sdk):
    """The SDK's lib directory and the archives link-archive links a program against."""
    lib = sdk / "lib"
    archives = {
        "crt0_linked": lib / "crt0_linked.o",
        "libxila_posix": lib / "libxila_posix.a",
        "libxila": lib / "libxila.a",
        "libc": lib / "libc.a",
    }
    missing = [path for path in archives.values() if not path.exists()]
    if missing:
        sys.exit(f"error: {', '.join(str(path) for path in missing)} not found: "
                 f"pass --sdk the directory of an SDK built by `build.py build`")
    return archives


def resolve_builtins_command(target):
    """Return the first available builtins-query command for `target`, or its primary command."""
    config = TARGETS.get(target, {})
    primary = config.get("builtins")
    if not primary:
        return None
    candidates = [primary, *config.get("builtins_fallbacks", ())]
    for command in candidates:
        if shutil.which(str(command[0])) is not None:
            return command
    return primary


def sdk_builtins(sdk, extra):
    """The compiler runtime archives of the SDK's target, when it has one (Arm's libgcc,
    RISC-V's libgcc, Xtensa's libgcc), plus any --builtins paths. They join the link group
    like the SDK's own archives; on the host target there is none.

    When the target's cross compiler is not installed, its runtime is simply not bundled:
    a note goes to stderr and the caller (link-archive) proceeds without it, so the host
    or firmware must supply the builtins the program needs with --builtins."""
    builtins = []
    info = sdk / "sdk.json"
    if info.exists():
        target = json.loads(info.read_text()).get("target")
        command = resolve_builtins_command(target)
        if command:
            if shutil.which(str(command[0])) is None:
                print(f"note: {command[0]} not found: the compiler builtins of {target} are "
                      "not bundled; pass them with --builtins if the program needs them",
                      file=sys.stderr)
            else:
                try:
                    result = subprocess.run([str(part) for part in command],
                                            capture_output=True, text=True)
                except FileNotFoundError:
                    result = None
                if result is not None and result.returncode == 0:
                    path = Path(result.stdout.strip())
                    if path.is_file():
                        builtins.append(path)
    builtins.extend(Path(path) for path in extra)
    return builtins


def link_archive_plan(archives, entry, output, inputs, builtins, work, tools,
                      weak_undefined=()):
    """The commands `link-archive` runs, as data: combine every input with crt0_linked.o
    and the SDK archives into one relocatable object, resolve every weak undefined symbol
    to the absolute 0, rename the entry, localize every other symbol, and pack the result
    into the output archive. `tools` maps lld, objcopy and ar to their paths; every command
    is a list of strings.

    The resolve step matters: a weak undefined symbol the SDK archives did not define (the
    POSIX layer references its optional modules weakly) would otherwise stay a global
    undefined symbol in the archive, and a host link would satisfy it with any matching
    symbol, glibc's `stderr` in a Linux PIE test binary for example. Defined as the absolute
    0, it reads as null, exactly what a weak reference must see, and the localization below
    makes it local, so it can never bind outside the archive. Position-independent code
    (every SDK object is -fPIE) links the result into both PIE and non-PIE binaries."""
    combined = str(work / "combined.o")
    resolved = str(work / "resolved.o")
    renamed = str(work / "renamed.o")
    localized = str(work / f"{entry}.o")
    group = [str(archives["libxila_posix"]), str(archives["libxila"]), str(archives["libc"]),
             *[str(path) for path in builtins]]
    return [
        [tools["lld"], "-r", "-o", combined, "--undefined=xila_linked_start",
         str(archives["crt0_linked"]), *[str(path) for path in inputs],
         "--start-group", *group, "--end-group"],
        [tools["lld"], "-r", "-o", resolved,
         *[f"--defsym={name}=0" for name in weak_undefined], combined],
        # Rename first, then keep the new name global: llvm-objcopy matches
        # --keep-global-symbol against the name before --redefine-sym renames it.
        [tools["objcopy"], f"--redefine-sym=xila_linked_start={entry}", resolved, renamed],
        [tools["objcopy"], f"--keep-global-symbol={entry}", renamed, localized],
        [tools["ar"], "rcs", str(output), localized],
    ]


def link_archive(args):
    """Localize one C program into an archive whose only global defined symbol is its
    entry: link crt0_linked.o, the program and the SDK archives into one relocatable
    object, resolve every weak undefined symbol to 0 and reject any strong undefined
    symbol the firmware does not provide, then make every symbol except the entry local,
    so the program's libc, POSIX, crt0 and application globals never collide with the
    host's or another program's, and its weak references can never bind to a host symbol."""
    entry = validate_entry_symbol(args.entry)
    sdk = Path(args.sdk)
    archives = sdk_libraries(sdk)
    inputs = [Path(name) for name in args.inputs]
    for path in inputs:
        if not path.exists():
            sys.exit(f"error: input {path} not found")
    builtins = sdk_builtins(sdk, args.builtins)
    allow_undefined = FIRMWARE_DEFINED | set(args.allow_undefined)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    sdk_target = HOST_TARGET
    info = sdk / "sdk.json"
    if info.exists():
        sdk_target = json.loads(info.read_text()).get("target", HOST_TARGET)
    tools = {
        "lld": find_target_tool(sdk_target, "lld"),
        "objcopy": find_target_tool(sdk_target, "objcopy"),
        "ar": find_target_tool(sdk_target, "ar"),
        "nm": find_target_tool(sdk_target, "nm"),
        "readobj": find_target_tool(sdk_target, "readobj"),
    }
    with tempfile.TemporaryDirectory(prefix=".link-archive-", dir=output.parent) as work:
        work = Path(work)
        archive = work / "program.a"
        run(link_archive_plan(archives, entry, archive, inputs, builtins, work, tools)[0])
        scan_constructors([work / "combined.o"], sdk_target, linked=True)
        weak, strong = undefined_symbols(work / "combined.o", tools["nm"])
        unexpected = sorted(name for name in strong if name not in allow_undefined)
        if unexpected:
            sys.exit("error: the program leaves strong undefined symbols the firmware does "
                     "not provide: " + ", ".join(unexpected) +
                     " (pass --allow-undefined <symbol> to accept one explicitly)")
        bad_relocs = unsafe_zero_relocations(work / "combined.o", weak, tools["readobj"])
        if bad_relocs:
            details = ", ".join(f"{sym} ({rel})" for sym, rel in bad_relocs)
            sys.exit("error: weak undefined symbol(s) have direct call/PC-relative relocations "
                     "that cannot resolve to absolute 0 in a PIE link: " + details +
                     " (provide a weak hidden default definition instead)")
        plan = link_archive_plan(archives, entry, archive, inputs, builtins, work, tools,
                                 weak_undefined=weak)
        for command in plan[1:]:
            run(command)
        remaining_weak, remaining_strong = undefined_symbols(work / f"{entry}.o", tools["nm"])
        leftover = remaining_weak + sorted(n for n in remaining_strong if n not in allow_undefined)
        if leftover:
            sys.exit("error: localization left undefined symbols in the archive: " +
                     ", ".join(leftover))
        # Replace the output so a different entry cannot leave an old archive member.
        archive.replace(output)
    print(f"linked archive: {output} (entry: {entry})")


def resource_dir(target):
    config = TARGETS[target]
    out = subprocess.run(["clang", f"--target={config['triple']}", "-print-resource-dir"],
                         capture_output=True, text=True, check=True)
    return out.stdout.strip()


def rust_sdk_files():
    """Return the standalone Rust sources, with paths relative to the SDK's rust directory."""
    sources = [RUST_CRATE / name for name in RUST_CRATE_FILES]
    sources.extend(source for source in (RUST_CRATE / "src").rglob("*") if source.is_file())
    files = {
        Path("xila-sys") / source.relative_to(RUST_CRATE): source.read_bytes()
        for source in sources
    }

    # Replace the checkout-relative dependency path with the bundled sibling crate. In the
    # checkout the crate is a member of Core's workspace (the executable tests use it), so its
    # manifest has no workspace table. The bundled copy sits outside that workspace and needs
    # its own, or Cargo would search the user's parent directories for one.
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]
    manifest = Path("xila-sys/Cargo.toml")
    bindings = files[manifest].decode()
    checkout_path = tomllib.loads(bindings)["dependencies"]["xila-version"]["path"]
    bindings = bindings.replace(f'path = "{checkout_path}"', 'path = "../xila-version"')
    # Resolve the fields the crate inherits from Core's workspace, so the bundled copy builds
    # with the empty `[workspace]` table appended below.
    for field in ("version", "edition", "rust-version"):
        bindings = re.sub(rf"^{field}\.workspace = true$", f'{field} = "{workspace[field]}"',
                          bindings, flags=re.M)
    files[manifest] = (bindings.rstrip("\n") + "\n\n[workspace]\n").encode()

    # Resolve the version crate's inherited fields without shipping the full Core workspace.
    version = (VERSION_CRATE / "Cargo.toml").read_text()
    for field in ("version", "edition", "rust-version"):
        version = re.sub(rf"^{field}\.workspace = true$", f'{field} = "{workspace[field]}"',
                         version, flags=re.M)
    files[Path("xila-version/Cargo.toml")] = version.encode()
    files[Path("xila-version/src/lib.rs")] = (VERSION_CRATE / "src/lib.rs").read_bytes()
    return files


def assemble(target, pin, install, crt0, crt0_linked, libraries, work):
    header = (SDK_SOURCE / "include/xila/application.h").read_text()
    major, minor = (
        int(re.search(rf"^#define XILA_VERSION_{part} (\d+)$", header, re.M).group(1))
        for part in ("MAJOR", "MINOR"))
    xila_version = f"{major}.{minor}"
    name = f"xila-sdk-{xila_version}-{target}"
    root = work / name
    # Files are overwritten in place: the build never deletes anything.
    (root / "include/xila").mkdir(parents=True, exist_ok=True)
    (root / "lib").mkdir(exist_ok=True)
    (root / "cmake").mkdir(exist_ok=True)
    for example in ("hello", "hexdump"):
        (root / "share/xila/example" / example).mkdir(parents=True, exist_ok=True)

    shutil.copytree(install / "include", root / "include", dirs_exist_ok=True)
    shutil.copytree(SDK_SOURCE / "sdk/posix/include", root / "include", dirs_exist_ok=True)
    shutil.copy(SDK_SOURCE / "include/xila/application.h", root / "include/xila/")
    shutil.copy(SDK_SOURCE / "include/xila/graphics.h", root / "include/xila/")
    shutil.copy(SDK_SOURCE / "sdk/stubs.h", root / "include/xila/")
    shutil.copy(crt0, root / "lib/crt0.o")
    shutil.copy(crt0_linked, root / "lib/crt0_linked.o")
    for archive in ("libxila.a", "libxila_posix.a"):
        shutil.copy(libraries / archive, root / "lib" / archive)
    shutil.copy(install / "lib/libc.a", root / "lib/libc.a")
    shutil.copy(install / "lib/libm.a", root / "lib/libm.a")  # empty: libm is inside libc.a
    linker_script = SPLIT_LINKER_SCRIPT if TARGETS[target].get("split_rodata") else LINKER_SCRIPT
    shutil.copy(linker_script, root / "lib/xila-static-pie.ld")
    for example in ("hello", "hexdump"):
        for file in (SDK_SOURCE / "sdk/example" / example).iterdir():
            shutil.copy(file, root / "share/xila/example" / example / file.name)
    shutil.copy(SDK_SOURCE / "sdk/README.md", root / "README.md")
    for relative, contents in rust_sdk_files().items():
        destination = root / "rust" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)

    config = TARGETS[target]
    is_gcc = bool(config.get("compiler"))
    compiler_bin = config["compiler"] if is_gcc else "clang"
    linker_bin = f"{config['tool_prefix']}ld" if config.get("tool_prefix") else "ld.lld"
    target_flag = "" if is_gcc else f"--target={config['triple']}"
    include_dir_query = "-print-file-name=include" if is_gcc else f"--target={config['triple']};-print-resource-dir"
    include_subdir = "" if is_gcc else "/include"
    builtins_command = resolve_builtins_command(target) or []
    template = (SDK_SOURCE / "sdk/cmake/xila-toolchain.cmake.in").read_text()
    replacements = {
        "@XILA_SDK_TARGET@": target,
        "@XILA_COMPILER@": compiler_bin,
        "@XILA_LINKER@": linker_bin,
        "@XILA_TARGET_FLAG@": target_flag,
        "@XILA_INCLUDE_DIR_QUERY@": include_dir_query,
        "@XILA_INCLUDE_SUBDIR@": include_subdir,
        "@XILA_CMAKE_PROCESSOR@": config["processor"],
        "@XILA_ARCH_FLAGS@": ";".join(config["flags"]),
        "@XILA_DATA_ALIGNMENT@": str(config["data_alignment"]),
        "@XILA_BUILTINS_COMMAND@": ";".join(builtins_command),
    }
    for key, value in replacements.items():
        template = template.replace(key, value)
    (root / "cmake/xila-toolchain.cmake").write_text(template)

    # Licences: picolibc's BSD terms travel with the library; Core's Apache-2.0 covers the rest.
    shutil.copy(pin_source(pin) / "COPYING.picolibc", root / "COPYING.picolibc")
    shutil.copy(ROOT / "license.md", root / "LICENSE-xila.md")

    compiler_version = subprocess.run([find_tool(compiler_bin), "--version"],
                                      capture_output=True, text=True, check=True)
    (root / "sdk.json").write_text(json.dumps({
        "name": name,
        "target": target,
        # The Xila version the SDK builds for. The key keeps its name for existing readers.
        "abi": xila_version,
        "picolibc": {"version": pin["version"], "sha256": pin["sha256"],
                     "options": PICOLIBC_OPTIONS},
        "compiler": compiler_version.stdout.splitlines()[0],
        "data_alignment": config["data_alignment"],
        "entry": "_xila_start -> xila_main -> main",
        "linked_entry": "xila_linked_start -> xila_main -> main",
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
    drop_members(install, target)
    crt0, crt0_linked, libraries = compile_sdk(target, install, work)
    name, root = assemble(target, pin, install, crt0, crt0_linked, libraries, work)
    scan_constructors([root / "lib/crt0.o", root / "lib/crt0_linked.o",
                       *sorted((root / "lib").glob("*.a"))], target=target)
    archive = pack(name, root)
    print(f"sdk directory: {root}")
    print(f"sdk archive:   {archive}")
    print(f"sha256:        {hashlib.sha256(archive.read_bytes()).hexdigest()}")


def map_sizes(text):
    """Bytes per origin and output section from an lld map: {origin: {section: size}}.
    Origins are the application object, crt0, and each SDK archive.

    The map has fixed columns (VMA LMA Size Align Out In Symbol): an output section starts in
    the "Out" column, an input section in "In", a symbol after that. Labels such as RISC-V's
    `.Lpcrel_hiN` also start with a dot, so the column decides, never the name."""
    sizes = {}
    output_column = None
    output_section = None
    rows = []
    for line in text.splitlines():
        if output_column is None and re.search(r"\bOut\s+In\s+Symbol\b", line):
            output_column = line.index("Out")
        match = re.match(r"(\s+[0-9a-f]+\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s+)(\S.*)$", line)
        if match:
            rows.append((len(match.group(1)), int(match.group(2), 16), match.group(3)))
    if output_column is None and rows:
        # No header (an excerpt): the output sections are the leftmost text.
        output_column = min(column for column, _size, _rest in rows)
    for column, size, rest in rows:
        if column <= output_column:
            output_section = rest.split()[0]
            continue
        member = re.match(r"(.+?):\((\.\S+)\)$", rest)
        if member is None:
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


def check_target(data, target):
    """check_elf against the target's machine and float ABI."""
    from check_elf import check

    config = TARGETS[target]
    return check(data, config["machine"], float_abi=config["float_abi"])


def verify_riscv_architecture(image, target):
    """Compare the linker-merged architecture with the firmware's extension set."""
    readelf = find_target_tool(target, "readelf")
    output = subprocess.check_output([readelf, "--arch-specific", image], text=True)
    architecture = re.search(r"Value:\s+(rv32\S+)", output)
    if not architecture:
        sys.exit(f"{image}: missing RISC-V architecture attribute")
    extensions = set()
    for extension in architecture[1].removeprefix("rv32").split("_"):
        match = re.fullmatch(r"([a-z][a-z0-9]*?)[0-9]+p[0-9]+", extension)
        if not match:
            sys.exit(f"{image}: invalid RISC-V extension {extension}")
        extensions.add(match[1])
    supported = set("imac")
    if TARGETS[target]["float_abi"] == "single":
        supported.add("f")
    policy = ROOT / "libraries/native_loader/riscv_extensions.txt"
    # See riscv_extensions.txt for the rule format, shared with the loader build script.
    for line in policy.read_text().splitlines():
        names = line.split("#", 1)[0].split()
        if not names:
            continue
        extension, *requirements = names
        if supported.issuperset(requirements):
            supported.add(extension)
    if unsupported := extensions - supported:
        sys.exit(f"{image}: RISC-V firmware lacks {', '.join(sorted(unsupported))}")


def verify_image(image, target):
    problems = check_target(image.read_bytes(), target)
    for problem in problems:
        print(f"{image}: {problem}", file=sys.stderr)
    if problems:
        sys.exit(1)
    if TARGETS[target]["machine"] == "riscv32":
        verify_riscv_architecture(image, target)
    print(f"{image}: ELF checks passed")


def verify_c_program(target, sdk, name, source, *options):
    build = OUTPUT / target / f"example-{name}"
    # --fresh (CMake 3.24): the cache keeps the first configure's flags, so an SDK rebuilt
    # with different target flags would otherwise leave the example on the old ones.
    run(["cmake", "--fresh", "-S", source, "-B", build,
         f"-DCMAKE_TOOLCHAIN_FILE={sdk / 'cmake/xila-toolchain.cmake'}",
         "-DCMAKE_BUILD_TYPE=MinSizeRel", *options])
    run(["cmake", "--build", build, "--target", name])
    image = build / f"{name}.elf"
    verify_image(image, target)
    return image


def link_packaged_example(target, sdk, name, archive):
    """Compile and localize an example from the SDK directory."""
    manifest = sdk / "sdk.json"
    try:
        metadata = json.loads(manifest.read_text())
    except (OSError, ValueError) as error:
        sys.exit(f"error: cannot read the SDK manifest at {manifest}: {error}. "
                 "Pass a built SDK directory to --sdk.")
    actual_target = metadata.get("target") if isinstance(metadata, dict) else None
    if not isinstance(actual_target, str) or not actual_target:
        sys.exit(f"error: SDK manifest {manifest} must contain a non-empty 'target' string. "
                 "Pass a built SDK directory to --sdk.")
    if actual_target != target:
        sys.exit(f"error: {sdk} targets {actual_target}, expected {target}")
    include = [
        "-nostdinc",
        f"-isystem{sdk / 'include/posix'}",
        f"-isystem{sdk / 'include'}",
        f"-isystem{compiler_include_dir(target)}",
    ]
    archive.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".link-example-", dir=archive.parent) as work:
        obj = Path(work) / f"{name}.o"
        run([*compiler_command(target), *COMMON_FLAGS, *include,
             "-c", sdk / "share/xila/example" / name / f"{name}.c", "-o", obj])
        link_archive(argparse.Namespace(sdk=str(sdk), entry=f"xila_linked_{name}",
                                        output=str(archive), builtins=[],
                                        allow_undefined=[], inputs=[str(obj)]))


def parse_nm_defined(text):
    """Parse `llvm-nm --extern-only --defined-only --format=posix` output into {name: kind}.

    Excludes `N` (debugging labels such as `.Ldebug_info0` or `.LLST0` from GCC's `libgcc.a`)
    and undefined symbols (`U`), while keeping all global defined classes including `C`
    (common), `i` (indirect function) and `u` (unique global)."""
    defined = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue  # an archive member header
        name, kind = parts[0], parts[1]
        if kind != "N" and kind != "U" and (kind.isupper() or kind in ("i", "u")):
            defined[name] = kind
    return defined


def nm_archive_symbols(archive, nm):
    """(global defined {name: kind}, weak undefined, strong undefined) of an archive."""
    defined_output = subprocess.run(
        [nm, "--extern-only", "--defined-only", "--format=posix", str(archive)],
        capture_output=True, text=True, check=True).stdout
    defined = parse_nm_defined(defined_output)
    weak, strong = undefined_symbols(archive, nm)
    return defined, weak, strong


def verify_linked_example(target, sdk, name):
    """Build lib<name>.a from the example's object with link-archive (entry
    xila_linked_<name>), check that the entry is the only global defined symbol and that
    no weak or unexpected undefined symbol remains, and report the localized size."""
    entry = f"xila_linked_{name}"
    work = OUTPUT / target / f"example-{name}-linked"
    archive = work / f"lib{name}.a"
    link_packaged_example(target, sdk, name, archive)
    defined, weak, strong = nm_archive_symbols(archive, find_target_tool(target, "nm"))
    if defined != {entry: "T"}:
        sys.exit(f"error: {archive}: global defined symbols are {defined}, expected only "
                 f"{entry} (T)")
    leftover = weak + sorted(symbol for symbol in strong if symbol not in FIRMWARE_DEFINED)
    if leftover:
        sys.exit(f"error: {archive}: undefined symbols remain: {', '.join(leftover)}")
    run([find_target_tool(target, "size"), archive])
    print(f"{archive}: {archive.stat().st_size} bytes on disk; the only global defined symbol "
          f"is {entry} (T), no weak and no unexpected undefined symbol remains")


def verify_rust_crate(sdk):
    """Check the archived Rust sources and build them outside the checkout."""
    prefix = f"{sdk.name}/rust"
    expected = {prefix: None}
    for relative, contents in rust_sdk_files().items():
        expected[f"{prefix}/{relative.as_posix()}"] = contents
        for parent in relative.parents:
            if parent != Path("."):
                expected.setdefault(f"{prefix}/{parent.as_posix()}", None)

    archive = OUTPUT / f"{sdk.name}.tar.zst"
    differences = []
    with tempfile.TemporaryDirectory(prefix="xila-rust-sdk-") as temporary:
        with subprocess.Popen(["zstd", "-dc", archive], stdout=subprocess.PIPE) as decompressor:
            with tarfile.open(fileobj=decompressor.stdout, mode="r|") as tar:
                for member in tar:
                    if member.name != prefix and not member.name.startswith(f"{prefix}/"):
                        continue
                    if member.name not in expected:
                        differences.append(f"unexpected {member.name}")
                        continue
                    contents = expected.pop(member.name)
                    if contents is None and member.isdir():
                        continue
                    if contents is not None and member.isfile():
                        archived = tar.extractfile(member).read()
                        if archived == contents:
                            destination = Path(temporary) / member.name.removeprefix(f"{prefix}/")
                            destination.parent.mkdir(parents=True, exist_ok=True)
                            destination.write_bytes(archived)
                            continue
                    differences.append(f"differs: {member.name}")
            # The decompressor may still have tar padding buffered in its output.
            decompressor.stdout.read()
            if decompressor.wait() != 0:
                sys.exit(f"error: could not decompress {archive}")
        differences.extend(f"missing {name}" for name in sorted(expected))
        if differences:
            sys.exit(f"error: Rust SDK sources in {archive} differ from the checkout:\n"
                     + "\n".join(differences))
        run(["cargo", "check", "--offline", "--locked", "--features", "runtime",
             "--target-dir", Path(temporary) / "target"], cwd=Path(temporary) / "xila-sys")
    print(f"{archive}: Rust SDK sources match the checkout and build offline")


def verify(args):
    """Verify the archive's Rust crate and the C examples' ELF contract, and report sizes."""
    target = args.target
    sdk = sdk_directory(target)
    verify_rust_crate(sdk)
    map_file = OUTPUT / target / "example-hello/hello.map"
    image = verify_c_program(target, sdk, "hello", sdk / "share/xila/example/hello",
                             f"-DCMAKE_EXE_LINKER_FLAGS=-Map={map_file}")
    # The main probe references environ. The accessor-only probe still calls setenv.
    probe_source = SDK_SOURCE / "sdk/test/probe"
    for name, needs_environ in (("probe", True), ("no-environ", False)):
        probe_map = OUTPUT / target / f"example-{name}" / f"{name}.map"
        verify_c_program(target, sdk, name, probe_source, f"-DCMAKE_EXE_LINKER_FLAGS=-Map={probe_map}")
        links_builder = "libxila_posix.a(environ.o)" in probe_map.read_text()
        if links_builder != needs_environ:
            sys.exit(f"error: unexpected environ builder linkage in {name}.elf")
        print(f"{name}.elf: environ builder {'present' if links_builder else 'absent'}")
    for name in ("threads", "ssp", "graphics", "lifecycle"):
        verify_c_program(target, sdk, name, SDK_SOURCE / "sdk/test" / name)
    verify_c_program(target, sdk, "lifecycle-direct", SDK_SOURCE / "sdk/test/lifecycle")
    if target == HOST_TARGET:
        verify_c_program(target, sdk, "lifecycle-cxx", SDK_SOURCE / "sdk/test/lifecycle")
    # The hexdump example, in both of its forms: the ELF the loader runs, and the
    # localized archive link-archive produces for a firmware that links it in.
    size_tool = find_target_tool(target, "size")
    hexdump = verify_c_program(target, sdk, "hexdump", sdk / "share/xila/example/hexdump")
    run([size_tool, hexdump])
    print(f"{hexdump}: ELF file {hexdump.stat().st_size} bytes")
    verify_linked_example(target, sdk, "hexdump")
    run([size_tool, image])
    print("\nBytes by origin (map):")
    sizes = map_sizes(map_file.read_text())
    print(f"{'origin':<18}{'.text':>8}{'.rodata':>9}{'.data+.got':>12}{'.bss':>7}")
    for origin, sections in sorted(sizes.items()):
        data = sections.get(".data", 0) + sections.get(".got", 0)
        print(f"{origin:<18}{sections.get('.text', 0):>8}{sections.get('.rodata', 0):>9}"
              f"{data:>12}{sections.get('.bss', 0):>7}")
    run([size_tool, "-t", "libxila_posix.a", "libxila.a", "crt0.o"], cwd=sdk / "lib")


def check_data_alignment(image, elf, alignment):
    """The data segment must start at the end of the text segment rounded up to the
    target's alignment: exactly where XILA_DATA_ALIGN puts it. A vaddr that is merely
    a multiple of the alignment would pass even if the alignment were not applied."""
    from check_elf import PF_W, PT_LOAD

    loads = [segment for segment in elf.segments() if segment["type"] == PT_LOAD]
    text = [segment for segment in loads if not segment["flags"] & PF_W]
    writable = [segment for segment in loads if segment["flags"] & PF_W]
    if not text or not writable:
        sys.exit(f"{image}: expected a text PT_LOAD followed by a writable PT_LOAD")
    end = max(segment["vaddr"] + segment["memsz"] for segment in text)
    expected = (end + alignment - 1) // alignment * alignment
    first = min(segment["vaddr"] for segment in writable)
    if first != expected:
        sys.exit(f"{image}: the first writable PT_LOAD starts at {first:#x}, expected {expected:#x}"
                 f" (the text end {end:#x} rounded up to {alignment} bytes)")


def rust_example(args):
    """Build an independent PIC sysroot for each selected Rust guest and verify its image."""
    from check_elf import Elf, SHT_REL, SHT_RELA

    nightly = tomllib.loads((ROOT / "mise.toml").read_text())["vars"]["nightly_version"]
    example = SDK_SOURCE / "sdk/rust/hello"
    output = OUTPUT / "rust-hello"
    # Core's target rustflags take precedence over the example's build.rustflags.
    # Pass the example's flags explicitly to override the host's mold driver option.
    config = tomllib.loads((example / ".cargo/config.toml").read_text())
    env = dict(os.environ, CARGO_ENCODED_RUSTFLAGS="\x1f".join(config["build"]["rustflags"]))
    selected = (
        (args.target,)
        if args.target
        else [name for name, cfg in TARGETS.items() if cfg.get("rust_example", True)]
    )
    for target in selected:
        run(["rustup", "run", nightly, "cargo", "build", "--locked", "--release",
             "--target", target, "--target-dir", output], cwd=example, env=env)
        image = output / target / "release/xila-rust-hello"
        verify_image(image, target)
        elf = Elf(image.read_bytes())
        # The fat-LTO hello exercises linking and loading without .rel.dyn/.rela.dyn.
        if any(section["size"] and section["type"] in (SHT_REL, SHT_RELA)
               for section in elf.sections()):
            sys.exit(f"{image}: the zero-relocation regression fixture has relocations")
        check_data_alignment(image, elf, TARGETS[target]["data_alignment"])
        run([find_tool("llvm-size"), image])
        print(f"ELF file: {image.stat().st_size} bytes")

    if HOST_TARGET in selected:
        lifecycle_output = OUTPUT / "rust-lifecycle"
        run(["rustup", "run", nightly, "cargo", "build", "--locked", "--release",
             "--target", HOST_TARGET, "--target-dir", lifecycle_output,
             "--features", "lifecycle-test"], cwd=example, env=env)
        image = lifecycle_output / HOST_TARGET / "release/xila-rust-hello"
        verify_image(image, HOST_TARGET)
        check_data_alignment(image, Elf(image.read_bytes()), TARGETS[HOST_TARGET]["data_alignment"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build", help="build the SDK directory and archive")
    build_parser.add_argument("--target", default=HOST_TARGET, choices=sorted(TARGETS))
    build_parser.set_defaults(function=build)
    verify_parser = commands.add_parser("verify", help="build the example and check the ELF")
    verify_parser.add_argument("--target", default=HOST_TARGET, choices=sorted(TARGETS))
    verify_parser.set_defaults(function=verify)
    rust_parser = commands.add_parser("rust-example", help="build and check the Rust static PIE example")
    rust_parser.add_argument("--target", choices=sorted(TARGETS), help="build only this target")
    rust_parser.set_defaults(function=rust_example)
    archive_parser = commands.add_parser(
        "link-archive",
        help="localize one linked C program into an archive with a single entry symbol")
    archive_parser.add_argument("--sdk", required=True,
                                help="the SDK directory (xila-sdk-<version>-<target>)")
    archive_parser.add_argument("--entry", required=True,
                                help="the program's entry symbol, a C identifier")
    archive_parser.add_argument("--output", required=True, help="the archive to write (<lib.a>)")
    archive_parser.add_argument("--builtins", action="append", default=[], metavar="ARCHIVE",
                                help="an extra compiler runtime archive for the link group "
                                     "(repeatable)")
    archive_parser.add_argument("--allow-undefined", action="append", default=[],
                                metavar="SYMBOL",
                                help="a strong undefined symbol the firmware provides "
                                     "(repeatable); anything else is an error")
    archive_parser.add_argument("inputs", nargs="+", metavar="OBJECT",
                                help="the program's object files or archives")
    archive_parser.set_defaults(function=link_archive)
    example_parser = commands.add_parser("link-example", help="build a linked SDK example archive")
    example_parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    example_parser.add_argument("--sdk", required=True, type=Path)
    example_parser.add_argument("--output", required=True, type=Path)
    example_parser.add_argument("name", choices=("hello", "hexdump"))
    example_parser.set_defaults(function=lambda args: link_packaged_example(
        args.target, args.sdk, args.name, args.output))
    args = parser.parse_args()
    if args.command != "rust-example":
        # Put the versioned LLVM directory on PATH for Meson and Clang's linker probes,
        # when lld is installed.
        lld = shutil.which("ld.lld")
        if not lld and shutil.which("clang"):
            candidate = subprocess.run(["clang", "-print-prog-name=ld.lld"],
                                       capture_output=True, text=True).stdout.strip()
            if candidate and os.path.isabs(candidate) and os.path.exists(candidate):
                lld = candidate
        if lld:
            os.environ["PATH"] = os.path.dirname(lld) + os.pathsep + os.environ["PATH"]
    args.function(args)


if __name__ == "__main__":
    main()
