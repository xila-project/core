#!/usr/bin/env bash
# Links the Xtensa static C archives (LVGL, font assets, LittleFS) across the
# ESP32-S3 flash-to-ROM address gap (0x42000000 to 0x40000000) to verify that
# every C object was built with -mlongcalls instead of 18-bit call8 branches.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "error: expected Cargo build-script output directories" >&2
  exit 2
fi

lvgl_archive=""
font_archive=""
littlefs_archive=""
for directory in "$@"; do
  [ -f "$directory/liblvgl.a" ] && lvgl_archive="$directory/liblvgl.a"
  [ -f "$directory/libfont_assets.a" ] && font_archive="$directory/libfont_assets.a"
  [ -f "$directory/liblfs-sys.a" ] && littlefs_archive="$directory/liblfs-sys.a"
done

for pair in \
  "liblvgl.a:$lvgl_archive" \
  "libfont_assets.a:$font_archive" \
  "liblfs-sys.a:$littlefs_archive"; do
  name=${pair%%:*}
  path=${pair#*:}
  if [ -z "$path" ]; then
    echo "error: this run built no $name for xtensa-esp32s3-none-elf" >&2
    exit 1
  fi
done

font_symbol=$(xtensa-esp32s3-elf-nm --defined-only -g "$font_archive" | awk '/ [TDGR] /{print $3; exit}')
if [ -z "$font_symbol" ]; then
  echo "error: could not find an exported symbol in $font_archive" >&2
  exit 1
fi

libgcc=$(xtensa-esp32s3-elf-gcc -print-libgcc-file-name)
target_root=${CARGO_TARGET_DIR:-target}
mkdir -p "$target_root"
work=$(mktemp -d "$target_root/xtensa-link-check.XXXXXXXX")
trap 'rm -rf "$work"' EXIT

cat >"$work/link.ld" <<'EOF'
ENTRY(_probe)
SECTIONS {
  .rom 0x40000000 : {
    PROVIDE(memcpy = 0x40000000);
    PROVIDE(memset = 0x40000010);
    PROVIDE(memmove = 0x40000020);
    PROVIDE(memcmp = 0x40000030);
    PROVIDE(strlen = 0x40000040);
    PROVIDE(strcmp = 0x40000050);
    PROVIDE(strcpy = 0x40000060);
    PROVIDE(strncpy = 0x40000070);
    PROVIDE(strcat = 0x40000080);
    PROVIDE(strchr = 0x40000090);
    PROVIDE(strrchr = 0x400000a0);
    PROVIDE(strstr = 0x400000b0);
    PROVIDE(abs = 0x400000c0);
    PROVIDE(__assert_func = 0x400000d0);
  }
  .text 0x42000000 : { *(.literal* .text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
}
EOF

cat >"$work/probe.c" <<EOF
extern void lv_init(void);
extern void *lv_obj_create(void *parent);
extern int lfs_mount(void *lfs, const void *cfg);
extern const char $font_symbol;

void _probe(void) {
  lv_init();
  lv_obj_create(0);
  lfs_mount(0, 0);
  (void)$font_symbol;
}

void lv_mem_init(void) {}
void *lv_malloc_core(unsigned int size) { (void)size; return 0; }
void lv_free_core(void *pointer) { (void)pointer; }
void *lv_realloc_core(void *pointer, unsigned int size) { (void)pointer; (void)size; return 0; }
void *lv_memcpy(void *destination, const void *source, unsigned int size) { (void)source; (void)size; return destination; }
void *lv_memset(void *destination, int value, unsigned int size) { (void)value; (void)size; return destination; }
void *lv_memmove(void *destination, const void *source, unsigned int size) { (void)source; (void)size; return destination; }
unsigned int lv_strlen(const char *string) { (void)string; return 0; }
int lv_strcmp(const char *left, const char *right) { (void)left; (void)right; return 0; }
char *lv_strdup(const char *string) { (void)string; return 0; }
void lv_obj_assign_id(const void *class_pointer, void *object) { (void)class_pointer; (void)object; }
void lv_obj_free_id(void *object) { (void)object; }
unsigned int lfs_crc(unsigned int crc, const void *buffer, unsigned int size) { (void)buffer; (void)size; return crc; }
void *lfs_malloc(unsigned int size) { (void)size; return 0; }
void lfs_free(void *pointer) { (void)pointer; }
EOF

xtensa-esp32s3-elf-gcc -mlongcalls -c "$work/probe.c" -o "$work/probe.o"
xtensa-esp32s3-elf-ld -T "$work/link.ld" --gc-sections --no-warn-rwx-segments \
  -o "$work/probe.elf" "$work/probe.o" \
  --start-group "$lvgl_archive" "$font_archive" "$littlefs_archive" "$libgcc" --end-group
echo "Xtensa long-call link check passed: $lvgl_archive, $font_archive, $littlefs_archive"
