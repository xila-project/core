#include <unistd.h>

// GDB sets a breakpoint by name while this function is loaded.
__attribute__((noinline))
int xila_jit_probe(void) {
    char byte;
    if (read(STDIN_FILENO, &byte, 1) != 1)
        return 1;
    if (byte == 'e')
        _exit(8);
    return 7;
}

int main(void) {
    return xila_jit_probe();
}
