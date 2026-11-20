/* A worker calls exit(9) while main is suspended in a graphics call.
 *
 * Main builds a window and a button, then polls the window's events. A worker waits for the
 * host to create /go, then exits. The host holds the graphics lock across both, so main is
 * suspended waiting for it and exit has to wait for it too before it deletes the objects.
 * The program creates the directory /exit-ready when its objects exist.
 *
 * Failures return the number of the failed step. */
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>
#include <xila/graphics.h>
#include <xila/stubs.h>

static void *worker(void *argument) {
    (void)argument;
    struct stat st;
    while (stat("/go", &st) != 0)
        xila_time_sleep(1000000);
    exit(9);
    return NULL;
}

int main(void) {
    XilaGraphicsObject *window = NULL, *button = NULL;
    if (xila_graphics_window_create(&window) != 0) return 1;
    if (xila_graphics_button_create(window, &button) != 0) return 2;
    pthread_t thread;
    if (pthread_create(&thread, NULL, worker, NULL) != 0) return 3;
    if (mkdir("/exit-ready", 0755) != 0) return 4;
    for (;;) {
        uint32_t code = 0;
        uintptr_t target = 0;
        int32_t x, y, dx, dy;
        /* Poll; while the test holds the lock, main is suspended inside this call. */
        if (xila_graphics_window_pop_pointer_event(window, &code, &target, &x, &y, &dx, &dy) != 0)
            return 5;
        xila_time_sleep(1000000);
    }
}
