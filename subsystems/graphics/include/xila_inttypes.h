/* LVGL's LV_INTTYPES_INCLUDE. Wasm and bare-metal ARM may have no C library,
 * so their format macros are defined here from compiler-provided <stdint.h>.
 * LVGL formats with its own sprintf, where a mismatch between `int` and `long`
 * on these ILP32 targets is only a -Wformat warning, never a runtime error. */
#ifndef XILA_INTTYPES_H
#define XILA_INTTYPES_H

#if defined(__wasm__) || defined(__wasm32__) || (defined(__arm__) && !defined(__linux__))
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

#if defined(__INT32_FMTd__)
/* Clang names the exact types. */
#define PRId32 __INT32_FMTd__
#define PRIu32 __UINT32_FMTu__
#define PRIx32 __UINT32_FMTx__
#define PRIX32 __UINT32_FMTX__
#define PRId64 __INT64_FMTd__
#define PRIu64 __UINT64_FMTu__
#define PRIx64 __UINT64_FMTx__
#define PRIX64 __UINT64_FMTX__
#define PRIdPTR __INTPTR_FMTd__
#define PRIuPTR __UINTPTR_FMTu__
#define PRIxPTR __UINTPTR_FMTx__
#define PRIXPTR __UINTPTR_FMTX__
#else
#if defined(__arm__)
/* arm-none-eabi GCC: int32_t is long int, uintptr_t is unsigned int. */
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
#define PRId64 "lld"
#define PRIu64 "llu"
#define PRIx64 "llx"
#define PRIX64 "llX"
#define PRIdPTR "d"
#define PRIuPTR "u"
#define PRIxPTR "x"
#define PRIXPTR "X"
#endif

#else
#include <inttypes.h>
#endif

#endif /* XILA_INTTYPES_H */
