#if defined(__wasm__) || defined(__wasm32__)
#define PRId32 "d"
#define PRIu32 "u"
#define PRIx32 "x"
#define PRIX32 "X"

#define PRId64 "lld"
#define PRIu64 "llu"
#define PRIx64 "llx"
#define PRIX64 "llX"

#define PRIuPTR "u"
#else
#include <inttypes.h>
#endif
