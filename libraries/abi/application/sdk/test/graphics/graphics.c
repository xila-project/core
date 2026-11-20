/* Graphics through the native ABI: builds a few objects, sets properties, reads them back,
 * waits for a click on a button and leaves with them all alive.
 *
 * The program creates a window, a button with a label, a checkbox and a slider, checks what
 * the host reports for them, then polls the window's events until the host test clicks the
 * button. It never deletes anything: the host must delete the window and its objects when
 * the process ends, whether main returns or the program calls exit(). It creates the
 * directory /graphics-ready when its objects exist and removes it after the click.
 *
 *   graphics            return 0 from main
 *   graphics exit       call exit(7) from main
 *
 * Failures exit with the line of the failed check. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <xila/graphics.h>
#include <xila/stubs.h>

#define CHECK(condition)                                                                   \
    do {                                                                                   \
        if (!(condition)) {                                                                \
            fprintf(stderr, "graphics: check failed at line %d\n", __LINE__);              \
            return __LINE__;                                                               \
        }                                                                                  \
    } while (0)

#define CALL(call) CHECK((call) == 0)

/* Longer than any string the host accepts, and with no terminator. */
static char unterminated[4096];

int main(int argc, char **argv) {
    XilaGraphicsObject *window = NULL, *button = NULL, *label = NULL;
    XilaGraphicsObject *checkbox = NULL, *slider = NULL;

    CALL(xila_graphics_window_create(&window));
    CALL(xila_graphics_button_create(window, &button));
    CALL(xila_graphics_label_create(button, &label));
    CALL(xila_graphics_label_set_text(label, "Click me"));
    CALL(xila_graphics_checkbox_create(window, &checkbox));
    CALL(xila_graphics_checkbox_set_text(checkbox, "Enabled"));
    CALL(xila_graphics_slider_create(window, &slider));
    CALL(xila_graphics_range_set_range(slider, 0, 200));
    CALL(xila_graphics_range_set_value(slider, 120, false));

    /* Sizes and values come back through result pointers. */
    CALL(xila_graphics_object_set_size(button, 120, 40));
    CALL(xila_graphics_object_update_layout(window));
    int32_t width = 0, height = 0, value = 0;
    CALL(xila_graphics_object_get_width(button, &width));
    CALL(xila_graphics_object_get_height(button, &height));
    CALL(xila_graphics_range_get_value(slider, &value));
    CHECK(width == 120 && height == 40 && value == 120);

    /* A color argument is packed into one call word. */
    XilaGraphicsColor red = {.blue = 0, .green = 0, .red = 255};
    CALL(xila_graphics_window_set_icon(window, "R", red));

    /* The host reads strings with a bound: an unterminated one is refused, not scanned. */
    memset(unterminated, 'x', sizeof(unterminated));
    CHECK(xila_graphics_label_set_text(label, unterminated) == XILA_STATUS_OUT_OF_MEMORY);
    /* A handle that is not this process's object is refused, never dereferenced. */
    CHECK(xila_graphics_object_get_width((XilaGraphicsObject *)(uintptr_t)0x7fff, &width) != 0);
    /* A call without a process capability fails before it reaches graphics. */
    CHECK(xila_graphics_call(NULL, XILA_GRAPHICS_FUNCTION_CALL_WINDOW_CREATE, NULL, 0,
                             &width) == XILA_STATUS_INVALID_ARGUMENT);
    /* Wrong arguments never reach the executor's handlers. */
    CHECK(xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LABEL_SET_TEXT,
                             NULL, 0, NULL) == XILA_STATUS_INVALID_ARGUMENT);

    /* Tell the host the objects exist. A directory is visible to it at once, where the
     * contents of a file this program still has open are not. */
    CHECK(mkdir("/graphics-ready", 0755) == 0);

    /* Poll the window's events until the host test clicks the button. */
    uint64_t start = 0, now = 0;
    CHECK(xila_time_monotonic(&start) == XILA_STATUS_OK);
    for (;;) {
        uint32_t code = 0;
        uintptr_t target = 0;
        int32_t x = 0, y = 0, dx = 0, dy = 0;
        CALL(xila_graphics_window_pop_pointer_event(window, &code, &target, &x, &y, &dx, &dy));
        if (code == XILA_GRAPHICS_EVENT_CODE_CLICKED) {
            /* The target is this process's handle for the button, not an address. */
            CHECK(target == (uintptr_t)button);
            break;
        }
        if (code == 0) {
            CHECK(xila_time_sleep(1000000) == XILA_STATUS_OK);
            CHECK(xila_time_monotonic(&now) == XILA_STATUS_OK);
            CHECK(now - start < 10ULL * 1000000000ULL);
        }
    }

    CHECK(rmdir("/graphics-ready") == 0);
    printf("graphics: ok\n");
    fflush(stdout);
    if (argc > 1 && strcmp(argv[1], "exit") == 0)
        exit(7);
    return 0;
}
