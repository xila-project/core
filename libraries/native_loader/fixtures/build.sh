#!/bin/sh
# Build a separate static PIE with page-separated permissions for Linux mmap.
# XILA_FIXTURE_FLAGS adds compiler flags, for example -DXILA_FIXTURE_CONSTRUCTOR.
set -eu
fixture_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror -O2 -fPIE -ffreestanding \
    -fno-stack-protector -ffunction-sections -fdata-sections -pie -nostdlib \
    -Wl,--no-dynamic-linker,-Bsymbolic,-z,max-page-size=16,--gc-sections,--build-id=none \
    -Wl,--defsym=XILA_DATA_ALIGN=4096,-T,"$fixture_dir/../xila-static-pie.ld" \
    -I "$fixture_dir/../../abi/application/include" ${XILA_FIXTURE_FLAGS:-} "$fixture_dir/start.c" -o "$1"
