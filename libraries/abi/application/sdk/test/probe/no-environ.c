#include <stdlib.h>
#include <string.h>

int main(void) {
    if (setenv("SDK_PROBE", "value", 1) != 0)
        return 1;
    const char *value = getenv("SDK_PROBE");
    if (value == NULL || strcmp(value, "value") != 0)
        return 2;
    return unsetenv("SDK_PROBE") == 0 && getenv("SDK_PROBE") == NULL ? 0 : 3;
}
