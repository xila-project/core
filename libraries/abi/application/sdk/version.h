#ifndef XILA_SDK_VERSION_H
#define XILA_SDK_VERSION_H

#include <stdbool.h>
#include <stdint.h>

/* Whether a host running the packed Xila version `running` can run an image built for the
   packed version `declared`: before 1.0 the minor versions must be equal; from 1.0 the majors
   must be equal and the running minor at least the declared one. This mirrors
   xila_version::compatible in Core and xila_package_manifest::compatible in package_manager. */
static inline bool xila_version_compatible(uint32_t declared, uint32_t running) {
    uint32_t major = declared >> 16, minor = declared & 0xffff;
    if (major != running >> 16) return false;
    return major == 0 ? minor == (running & 0xffff) : minor <= (running & 0xffff);
}

#endif
