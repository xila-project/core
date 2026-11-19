/* LVGL's LV_INTTYPES_INCLUDE. Freestanding and bare-metal targets (Wasm, Arm,
 * RISC-V, Xtensa, ...) may have no C library, so their format macros are
 * defined here from compiler-provided <stdint.h>. Prefer the compiler's own
 * __*_FMT*__ macros (Clang) and fall back to GCC's __INT*_C literal suffixes
 * and pointer width so each target's actual `int` vs `long` types match. */
#ifndef XILA_INTTYPES_H
#define XILA_INTTYPES_H

#if (defined(__STDC_HOSTED__) && !__STDC_HOSTED__) || \
    (!defined(__unix__) && !defined(__linux__) && !defined(__APPLE__) && !defined(_WIN32))
#include <stdint.h>

/* 8- and 16-bit arguments are promoted to int. */
#define PRId8 "d"
#define PRIu8 "u"
#define PRIx8 "x"
#define PRIX8 "X"
#define PRId16 "d"
#define PRIu16 "u"
#define PRIx16 "x"
#define PRIX16 "X"

/* Distinguish `long` (`1L`) from `int` (`1`) or `long long` (`1LL`) via GCC's
 * `__INT32_C(1)` / `__INT64_C(1)` when `__*_FMT*__` macros are absent. */
#define XILA_INTTYPES_PASTE2(a, b) a##b
#define XILA_INTTYPES_PASTE(a, b) XILA_INTTYPES_PASTE2(a, b)
#define XILA_INTTYPES_IS_LONG_1 0
#define XILA_INTTYPES_IS_LONG_1L 1
#define XILA_INTTYPES_IS_LONG_1l 1
#define XILA_INTTYPES_IS_LONG_1LL 0
#define XILA_INTTYPES_IS_LONG_1ll 0

#if defined(__INT32_FMTd__)
/* Clang names the exact types. */
#define PRId32 __INT32_FMTd__
#define PRIu32 __UINT32_FMTu__
#define PRIx32 __UINT32_FMTx__
#define PRIX32 __UINT32_FMTX__
#else
#ifdef __INT32_C
#define XILA_INTTYPES_INT32_IS_LONG \
    XILA_INTTYPES_PASTE(XILA_INTTYPES_IS_LONG_, __INT32_C(1))
#else
#define XILA_INTTYPES_INT32_IS_LONG 0
#endif
#if XILA_INTTYPES_INT32_IS_LONG
#define PRId32 "ld"
#define PRIu32 "lu"
#define PRIx32 "lx"
#define PRIX32 "lX"
#else
#define PRId32 "d"
#define PRIu32 "u"
#define PRIx32 "x"
#define PRIX32 "X"
#endif
#undef XILA_INTTYPES_INT32_IS_LONG
#endif

#if defined(__INT64_FMTd__)
#define PRId64 __INT64_FMTd__
#define PRIu64 __UINT64_FMTu__
#define PRIx64 __UINT64_FMTx__
#define PRIX64 __UINT64_FMTX__
#else
#ifdef __INT64_C
#define XILA_INTTYPES_INT64_IS_LONG \
    XILA_INTTYPES_PASTE(XILA_INTTYPES_IS_LONG_, __INT64_C(1))
#else
#define XILA_INTTYPES_INT64_IS_LONG 0
#endif
#if XILA_INTTYPES_INT64_IS_LONG
#define PRId64 "ld"
#define PRIu64 "lu"
#define PRIx64 "lx"
#define PRIX64 "lX"
#else
#define PRId64 "lld"
#define PRIu64 "llu"
#define PRIx64 "llx"
#define PRIX64 "llX"
#endif
#undef XILA_INTTYPES_INT64_IS_LONG
#endif

#if defined(__INTPTR_FMTd__)
#define PRIdPTR __INTPTR_FMTd__
#define PRIuPTR __UINTPTR_FMTu__
#define PRIxPTR __UINTPTR_FMTx__
#define PRIXPTR __UINTPTR_FMTX__
#elif defined(__SIZEOF_POINTER__) && defined(__SIZEOF_LONG__) && (__SIZEOF_POINTER__ == 8)
#if __SIZEOF_LONG__ == 8
#define PRIdPTR "ld"
#define PRIuPTR "lu"
#define PRIxPTR "lx"
#define PRIXPTR "lX"
#else
#define PRIdPTR "lld"
#define PRIuPTR "llu"
#define PRIxPTR "llx"
#define PRIXPTR "llX"
#endif
#elif defined(__LP64__) || defined(_LP64)
#define PRIdPTR "ld"
#define PRIuPTR "lu"
#define PRIxPTR "lx"
#define PRIXPTR "lX"
#else
#define PRIdPTR "d"
#define PRIuPTR "u"
#define PRIxPTR "x"
#define PRIXPTR "X"
#endif

#undef XILA_INTTYPES_IS_LONG_1ll
#undef XILA_INTTYPES_IS_LONG_1LL
#undef XILA_INTTYPES_IS_LONG_1l
#undef XILA_INTTYPES_IS_LONG_1L
#undef XILA_INTTYPES_IS_LONG_1
#undef XILA_INTTYPES_PASTE
#undef XILA_INTTYPES_PASTE2

#else
#include <inttypes.h>
#endif

#endif /* XILA_INTTYPES_H */
