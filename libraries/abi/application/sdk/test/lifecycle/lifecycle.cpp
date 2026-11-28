#include <stdio.h>

static int initialized;

struct Lifecycle {
    Lifecycle() {
        puts("C++ constructor");
        initialized = 1;
    }

    ~Lifecycle() {
        puts("C++ destructor");
    }
};

static Lifecycle lifecycle;

int main(int, char **) {
    puts("C++ main");
    return initialized == 1 ? 23 : 1;
}
