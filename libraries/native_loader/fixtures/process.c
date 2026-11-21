#include <xila/application.h>

/* ELF note descriptor is the packed version of the header used by the app. */
__attribute__((section(".note.xila.abi"), aligned(4), used))
static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} abi_note = {5, 4, 1, "XILA", (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR};

/* The image links no libc, but the compiler still emits calls to these. */
void *memset(void *destination, int value, size_t count) {
    for (size_t index = 0; index < count; ++index) ((unsigned char *)destination)[index] = (unsigned char)value;
    return destination;
}
void *memcpy(void *destination, const void *source, size_t count) {
    for (size_t index = 0; index < count; ++index) ((unsigned char *)destination)[index] = ((const unsigned char *)source)[index];
    return destination;
}
int memcmp(const void *left, const void *right, size_t count) {
    for (size_t index = 0; index < count; ++index) {
        int difference = ((const unsigned char *)left)[index] - ((const unsigned char *)right)[index];
        if (difference != 0) return difference;
    }
    return 0;
}

#define CHECK(expression) do { if (!(expression)) return (int32_t)__LINE__; } while (0)

static size_t length(const char *text) {
    size_t count = 0;
    while (text[count] != 0) ++count;
    return count;
}

static int equal(XilaString text, const char *expected) {
    return text.length == length(expected) && memcmp(text.data, expected, text.length) == 0;
}

static uint32_t parse(XilaString text) {
    uint32_t value = 0;
    for (size_t index = 0; index < text.length; ++index) value = value * 10 + (uint32_t)(text.data[index] - '0');
    return value;
}

static size_t format(uint32_t value, char *buffer) {
    char reversed[10];
    size_t count = 0;
    do {
        reversed[count++] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);
    for (size_t index = 0; index < count; ++index) buffer[index] = reversed[count - 1 - index];
    return count;
}

/* The variable has this value, or is absent when `expected` is NULL. */
static int env_is(const XilaStartup *startup, const char *name, const char *expected) {
    char buffer[16];
    size_t count = 0;
    XilaStatus status = startup->api->env_get(startup->process, name, length(name), buffer, sizeof(buffer), &count);
    if (expected == NULL) return status == XILA_STATUS_NOT_FOUND;
    return status == XILA_STATUS_OK && count == length(expected) && memcmp(buffer, expected, count) == 0;
}

static int32_t exit_from_depth(const XilaStartup *startup, int depth) {
    if (depth == 0) {
        startup->api->process_exit(startup->process, 21);
        return 1; /* Exit returned. */
    }
    return exit_from_depth(startup, depth - 1) + 1;
}

static XilaSpawn spawn_with(const XilaStartup *startup) {
    XilaSpawn spawn;
    memset(&spawn, 0, sizeof(spawn));
    spawn.size = sizeof(spawn);
    spawn.standard_input = startup->standard_input;
    spawn.standard_output = startup->standard_output;
    spawn.standard_error = startup->standard_error;
    return spawn;
}

/* Spawn the fixture itself with arguments, wait, and return the child's status. */
static int32_t run_child(const XilaStartup *startup, const char *program, const XilaString *arguments, size_t count, XilaSpawn spawn) {
    XilaChild *child = NULL;
    int32_t status = 0;
    spawn.arguments = arguments;
    spawn.argument_count = count;
    if (startup->api->process_spawn(startup->process, startup->home, program, length(program), &spawn, &child) != XILA_STATUS_OK)
        return -1000;
    if (startup->api->process_wait(startup->process, child, 0, &status) != XILA_STATUS_OK) status = -1001;
    startup->api->process_child_close(child);
    return status;
}

/*
 * Argument 0 selects the role. The parent gets argument 1, the identifier of the task
 * that launched it, and has its home at the root directory. Children get the parent's
 * identifier as argument 1.
 */
int32_t _xila_start(const XilaStartup *startup) {
    if (startup->size != sizeof(*startup) || startup->argument_count < 1 || !startup->api || !startup->process)
        return 1;
    const XilaApi *api = startup->api;
    const XilaProcess *me = startup->process;
    XilaString role = startup->arguments[0];
    uint32_t id = 0, parent = 0;
    CHECK(api->process_id(me, &id) == XILA_STATUS_OK);
    CHECK(api->process_parent_id(me, &parent) == XILA_STATUS_OK);

    if (equal(role, "child-inherit")) {
        CHECK(startup->argument_count == 2 && parse(startup->arguments[1]) == parent);
        CHECK(env_is(startup, "FROM_HOST", "host") && env_is(startup, "FROM_PARENT", "1"));
        CHECK(env_is(startup, "AFTER_SPAWN", NULL));
        CHECK(api->env_set(me, "CHILD_ONLY", 10, "1", 1) == XILA_STATUS_OK);
        CHECK(startup->home != NULL);
        return 7;
    }
    if (equal(role, "child-exit")) {
        size_t count = 0;
        CHECK(api->file_write(startup->standard_output, "bye", 3, &count) == XILA_STATUS_OK && count == 3);
        return exit_from_depth(startup, 4);
    }
    if (equal(role, "child-explicit")) {
        CHECK(env_is(startup, "FROM_HOST", NULL) && env_is(startup, "FROM_PARENT", NULL) && env_is(startup, "X", "1"));
        CHECK(startup->home == NULL);
        return 9;
    }
    if (!equal(role, "parent")) return 2;

    CHECK(startup->argument_count == 2 && parse(startup->arguments[1]) == parent && id != parent);
    CHECK(startup->home != NULL);
    CHECK(env_is(startup, "FROM_HOST", "host"));
    CHECK(api->env_set(me, "FROM_PARENT", 11, "1", 1) == XILA_STATUS_OK);
    char identifier[10];
    XilaString arguments[2] = {{"child-inherit", 13}, {identifier, format(id, identifier)}};
    XilaSpawn spawn = spawn_with(startup);
    spawn.home = startup->home;

    /* Copy on spawn: a variable set after the spawn never reaches the child. */
    XilaChild *child = NULL;
    int32_t status = 0;
    spawn.arguments = arguments;
    spawn.argument_count = 2;
    CHECK(api->process_spawn(me, startup->home, "process.elf", 11, &spawn, &child) == XILA_STATUS_OK);
    CHECK(api->env_set(me, "AFTER_SPAWN", 11, "1", 1) == XILA_STATUS_OK);
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_OK && status == 7);
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_NO_CHILD);
    CHECK(api->process_child_close(child) == XILA_STATUS_OK);
    CHECK(env_is(startup, "CHILD_ONLY", NULL));

    /* A child that exits from a nested frame. */
    arguments[0].data = "child-exit";
    arguments[0].length = 10;
    CHECK(run_child(startup, "process.elf", arguments, 2, spawn_with(startup)) == 21);

    /* An explicit environment replaces the inherited one. */
    XilaString explicit_environment[1] = {{"X=1", 3}};
    XilaSpawn explicit_spawn = spawn_with(startup);
    explicit_spawn.environment = explicit_environment;
    explicit_spawn.environment_count = 1;
    arguments[0].data = "child-explicit";
    arguments[0].length = 14;
    CHECK(run_child(startup, "process.elf", arguments, 2, explicit_spawn) == 9);

    /* Failures: a missing file, a file without the execute permission and a bad ELF. */
    child = NULL;
    CHECK(api->process_spawn(me, startup->home, "missing.elf", 11, &spawn, &child) == XILA_STATUS_NOT_FOUND);
    CHECK(api->process_spawn(me, startup->home, "noexec.elf", 10, &spawn, &child) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(api->process_spawn(me, startup->home, "bad.elf", 7, &spawn, &child) == XILA_STATUS_EXEC_FORMAT);
    CHECK(child == NULL);
    return 73;
}
