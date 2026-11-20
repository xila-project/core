/* malloc family on the process heap granted through XilaStartup.heap.
 *
 * picolibc is built without its sbrk-based allocator, so these definitions replace it.
 * Every block comes from xila_heap_allocate and carries the heap's own bookkeeping:
 * the layer adds nothing per block and keeps no global free list. */
#include <stdlib.h>
#include <string.h>

#include "internal.h"

static XilaHeap *heap(void) {
    return xila_process.startup != NULL ? xila_process.startup->heap : NULL;
}

void *xila_heap_alloc(size_t size) {
    XilaHeap *process_heap = heap();
    void *allocation = NULL;
    if (process_heap == NULL) {
        errno = ENOMEM;
        return NULL;
    }
    XilaStatus status = xila_heap_allocate(process_heap, size, &allocation);
    if (status != XILA_STATUS_OK) {
        errno = ENOMEM;
        return NULL;
    }
    return allocation;
}

void xila_heap_release(void *pointer) {
    XilaHeap *process_heap = heap();
    if (pointer != NULL && process_heap != NULL)
        (void)xila_heap_free(process_heap, pointer);
}

void *malloc(size_t size) { return xila_heap_alloc(size); }

void free(void *pointer) { xila_heap_release(pointer); }

void *calloc(size_t count, size_t size) {
    size_t total;
    if (__builtin_mul_overflow(count, size, &total)) {
        errno = ENOMEM;
        return NULL;
    }
    void *allocation = xila_heap_alloc(total);
    if (allocation != NULL)
        memset(allocation, 0, total);
    return allocation;
}

void *realloc(void *pointer, size_t size) {
    XilaHeap *process_heap = heap();
    void *allocation = NULL;
    if (process_heap == NULL) {
        errno = ENOMEM;
        return NULL;
    }
    if (pointer != NULL && size == 0) {
        /* C leaves this implementation-defined: free and return NULL, like glibc. */
        (void)xila_heap_free(process_heap, pointer);
        return NULL;
    }
    XilaStatus status = xila_heap_reallocate(process_heap, pointer, size, &allocation);
    if (status != XILA_STATUS_OK) {
        errno = xila_errno_from_status(status);
        return NULL;
    }
    return allocation;
}

void *reallocarray(void *pointer, size_t count, size_t size) {
    size_t total;
    if (__builtin_mul_overflow(count, size, &total)) {
        errno = ENOMEM;
        return NULL;
    }
    return realloc(pointer, total);
}

/* The process heap guarantees XILA_HEAP_ALIGNMENT. Larger alignments would need a
   header the heap's free cannot see, so they are refused instead of leaking. */
void *aligned_alloc(size_t alignment, size_t size) {
    if (alignment == 0 || (alignment & (alignment - 1)) != 0) {
        errno = EINVAL;
        return NULL;
    }
    if (alignment > XILA_HEAP_ALIGNMENT) {
        errno = ENOMEM;
        return NULL;
    }
    return xila_heap_alloc(size);
}

void *memalign(size_t alignment, size_t size) { return aligned_alloc(alignment, size); }

int posix_memalign(void **output, size_t alignment, size_t size) {
    if (alignment < sizeof(void *) || (alignment & (alignment - 1)) != 0)
        return EINVAL;
    if (alignment > XILA_HEAP_ALIGNMENT)
        return ENOMEM;
    void *allocation = xila_heap_alloc(size);
    if (allocation == NULL)
        return ENOMEM;
    *output = allocation;
    return 0;
}
