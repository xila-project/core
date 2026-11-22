#include "application.h"
#include <string.h>

/* A failed check returns its line number, which the parent compares with the status it
 * expects from the child. */
#define CHECK(expression) do { if (!(expression)) return __LINE__; } while (0)
#define TEXT(literal) { (literal), sizeof(literal) - 1 }

typedef int32_t (*Entry)(const XilaStartup *);

/* Handle of a running child, shared with a sibling that must not be able to wait on it. */
static XilaChild *volatile shared_child;
/* The parent's process capability, which a sibling must not be able to use either. */
static const XilaProcess *volatile shared_process;

static XilaSpawn spawn_of(const XilaStartup *startup);

static int check_env(const XilaStartup *startup, const char *name, const char *expected) {
    const XilaApi *api = startup->api;
    char buffer[16];
    size_t length = 99;
    size_t name_length = strlen(name);
    if (expected == NULL)
        return api->env_get(startup->process, name, name_length, buffer, sizeof(buffer), &length) ==
                   XILA_STATUS_NOT_FOUND && length == 99;
    return api->env_get(startup->process, name, name_length, buffer, sizeof(buffer), &length) ==
               XILA_STATUS_OK &&
           length == strlen(expected) && memcmp(buffer, expected, length) == 0;
}

static int same(XilaString text, const char *expected) {
    return text.length == strlen(expected) && memcmp(text.data, expected, text.length) == 0;
}

/* Arguments: "one", "two", the parent's identifier in decimal. Inherits the parent's
 * environment, gets its home directory and returns normally. */
static int32_t child_inherit(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    CHECK(startup->process != NULL && startup->argument_count == 3);
    CHECK(same(startup->arguments[0], "one") && same(startup->arguments[1], "two"));
    uint32_t id = 0;
    uint32_t parent = 0;
    uint32_t expected = 0;
    for (size_t i = 0; i < startup->arguments[2].length; i++)
        expected = expected * 10 + (uint32_t)(startup->arguments[2].data[i] - '0');
    CHECK(api->process_id(startup->process, &id) == XILA_STATUS_OK);
    CHECK(api->process_parent_id(startup->process, &parent) == XILA_STATUS_OK);
    CHECK(parent == expected && id != parent);
    /* Copy on spawn: values at the time of the spawn, and the copy is private. */
    CHECK(check_env(startup, "SHARED", "parent"));
    CHECK(check_env(startup, "LATE", NULL));
    CHECK(api->env_set(startup->process, "CHILD_ONLY", 10, "1", 1) == XILA_STATUS_OK);
    CHECK(api->env_set(startup->process, "SHARED", 6, "child", 5) == XILA_STATUS_OK);
    /* The home capability works under the child's own task. */
    CHECK(startup->home != NULL);
    XilaFile *file = NULL;
    CHECK(api->file_open(startup->home, "child-home", 10, XILA_OPEN_WRITE | XILA_OPEN_CREATE,
                         &file) == XILA_STATUS_OK);
    CHECK(api->file_close(file) == XILA_STATUS_OK);
    return 11;
}

static int deep_exit(const XilaStartup *startup, int depth) {
    if (depth == 0) {
        startup->api->process_exit(startup->process, 42);
        return __LINE__; /* Exit returned. */
    }
    return deep_exit(startup, depth - 1) + 1;
}

/* Writes to its standard output, then exits from a nested frame. */
static int32_t child_exit(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    CHECK(startup->home == NULL);
    size_t count = 0;
    CHECK(api->file_write(startup->standard_output, "bye", 3, &count) == XILA_STATUS_OK);
    CHECK(api->env_set(startup->process, "CHILD_ONLY", 10, "1", 1) == XILA_STATUS_OK);
    return deep_exit(startup, 3);
}

/* Spawned with the explicit environment ONLY=1, DUP=a, DUP=b. */
static int32_t child_explicit(const XilaStartup *startup) {
    CHECK(startup->home == NULL && startup->argument_count == 0 && startup->arguments == NULL);
    CHECK(check_env(startup, "ONLY", "1") && check_env(startup, "DUP", "b"));
    CHECK(check_env(startup, "SHARED", NULL));
    return 9;
}

/* Spawned with an explicit, empty environment. */
static int32_t child_empty_environment(const XilaStartup *startup) {
    CHECK(check_env(startup, "SHARED", NULL) && check_env(startup, "ONLY", NULL));
    return 8;
}

static int32_t child_slow(const XilaStartup *startup) {
    CHECK(startup->api->time_sleep(30000000) == XILA_STATUS_OK);
    return 5;
}

/* Only the parent may wait for a child: this sibling cannot. Its capability is not the
 * running task's, so it cannot spawn through it either. */
static int32_t child_steal(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    int32_t status = 123;
    CHECK(shared_child != NULL && shared_process != NULL);
    CHECK(api->process_wait(startup->process, shared_child, XILA_WAIT_NO_HANG, &status) ==
          XILA_STATUS_NO_CHILD);
    CHECK(status == 123);
    XilaSpawn spawn = spawn_of(startup);
    XilaChild *stolen = NULL;
    CHECK(startup->home != NULL);
    CHECK(api->process_spawn(shared_process, startup->home, "exit", 4, &spawn, &stolen) ==
          XILA_STATUS_INVALID_ARGUMENT);
    CHECK(stolen == NULL);
    return 0;
}

/* Returns its own process identifier, which the parent compares with the child's. */
static int32_t child_identify(const XilaStartup *startup) {
    uint32_t id = 0;
    CHECK(startup->api->process_id(startup->process, &id) == XILA_STATUS_OK && id != 0);
    return (int32_t)id;
}

static int32_t child_returns_negative(const XilaStartup *startup) {
    (void)startup;
    return -3;
}

Entry process_test_entry(const char *name, size_t length) {
    static const struct {
        const char *name;
        Entry entry;
    } table[] = {
        {"inherit", child_inherit},
        {"exit", child_exit},
        {"explicit", child_explicit},
        {"empty", child_empty_environment},
        {"slow", child_slow},
        {"steal", child_steal},
        {"negative", child_returns_negative},
        {"identify", child_identify},
    };
    for (size_t i = 0; i < sizeof(table) / sizeof(table[0]); i++)
        if (strlen(table[i].name) == length && memcmp(table[i].name, name, length) == 0)
            return table[i].entry;
    return NULL;
}

static XilaSpawn spawn_of(const XilaStartup *startup) {
    XilaSpawn spawn;
    memset(&spawn, 0, sizeof(spawn));
    spawn.size = sizeof(spawn);
    spawn.standard_input = startup->standard_input;
    spawn.standard_output = startup->standard_output;
    spawn.standard_error = startup->standard_error;
    return spawn;
}

static int32_t run_child(const XilaStartup *startup, const char *program, const XilaSpawn *spawn) {
    const XilaApi *api = startup->api;
    XilaChild *child = NULL;
    int32_t status = 0;
    if (api->process_spawn(startup->process, startup->home, program, strlen(program), spawn,
                           &child) != XILA_STATUS_OK)
        return -1000;
    if (api->process_wait(startup->process, child, 0, &status) != XILA_STATUS_OK)
        status = -1001;
    api->process_child_close(child);
    return status;
}

static int uint_text(uint32_t value, char *buffer) {
    char reversed[10];
    int count = 0;
    do {
        reversed[count++] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);
    for (int i = 0; i < count; i++) buffer[i] = reversed[count - 1 - i];
    return count;
}

/*
 * The parent process: it runs on a coroutine, its home is the root directory and its
 * environment is SHARED=parent. The host's children are the entries above.
 */
int32_t process_parent_main(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    const XilaProcess *me = startup->process;
    CHECK(me != NULL && startup->home != NULL);
    CHECK(startup->size == sizeof(*startup) && api->size == sizeof(*api));

    /* Identity. */
    uint32_t id = 777;
    uint32_t parent = 777;
    CHECK(api->process_id(me, &id) == XILA_STATUS_OK && id != 777);
    CHECK(api->process_parent_id(me, &parent) == XILA_STATUS_OK && parent != 777);
    CHECK(api->process_id(NULL, &id) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_id(me, NULL) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_parent_id(me, NULL) == XILA_STATUS_INVALID_ARGUMENT);

    /* Environment: get, set, replace, remove and their errors. */
    char buffer[16];
    memset(buffer, '#', sizeof(buffer));
    size_t length = 99;
    CHECK(check_env(startup, "SHARED", "parent"));
    CHECK(api->env_set(me, "GREETING", 8, "hello", 5) == XILA_STATUS_OK);
    CHECK(api->env_get(me, "GREETING", 8, buffer, 5, &length) == XILA_STATUS_OK && length == 5);
    CHECK(memcmp(buffer, "hello#", 6) == 0); /* No terminator. */
    length = 99;
    CHECK(api->env_get(me, "GREETING", 8, buffer, 4, &length) == XILA_STATUS_BUFFER_TOO_SMALL);
    CHECK(length == 5 && buffer[4] == 'o');
    length = 99;
    CHECK(api->env_get(me, "GREETING", 8, NULL, 0, &length) == XILA_STATUS_BUFFER_TOO_SMALL);
    CHECK(length == 5);
    CHECK(api->env_set(me, "GREETING", 8, "bye", 3) == XILA_STATUS_OK);
    CHECK(check_env(startup, "GREETING", "bye"));
    CHECK(api->env_set(me, "GREETING", 8, NULL, 0) == XILA_STATUS_OK);
    CHECK(check_env(startup, "GREETING", ""));
    CHECK(api->env_remove(me, "GREETING", 8) == XILA_STATUS_OK);
    CHECK(check_env(startup, "GREETING", NULL));
    CHECK(api->env_remove(me, "GREETING", 8) == XILA_STATUS_OK); /* Already gone. */
    CHECK(api->env_get(me, "", 0, buffer, sizeof(buffer), &length) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_get(me, "A=B", 3, buffer, sizeof(buffer), &length) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_get(me, "SHARED", 6, NULL, 1, &length) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_get(me, "SHARED", 6, buffer, 16, NULL) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_get(NULL, "SHARED", 6, buffer, 16, &length) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_set(me, "A=B", 3, "x", 1) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_set(me, "A\0B", 3, "x", 1) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_set(me, "A", 1, "x\0y", 3) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_set(me, "A", 1, NULL, 1) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_set(me, "\xff", 1, "x", 1) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_remove(me, "", 0) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->env_remove(NULL, "A", 1) == XILA_STATUS_INVALID_ARGUMENT);

    /* A child inherits a copy of the parent's environment made at the spawn, so a variable
     * set after the spawn never reaches it; it also receives the arguments and the home
     * capability, and returns normally. */
    char parent_text[16];
    int parent_length = uint_text(id, parent_text);
    XilaString arguments[3] = {TEXT("one"), TEXT("two"), {parent_text, (size_t)parent_length}};
    XilaSpawn spawn = spawn_of(startup);
    spawn.arguments = arguments;
    spawn.argument_count = 3;
    spawn.home = startup->home;
    XilaChild *child = NULL;
    int32_t status = 0;
    CHECK(api->process_spawn(me, startup->home, "inherit", 7, &spawn, &child) == XILA_STATUS_OK);
    CHECK(api->env_set(me, "LATE", 4, "1", 1) == XILA_STATUS_OK);
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_OK && status == 11);
    CHECK(api->process_child_close(child) == XILA_STATUS_OK);
    CHECK(api->env_remove(me, "LATE", 4) == XILA_STATUS_OK);
    /* The child's changes never reach the parent. */
    CHECK(check_env(startup, "SHARED", "parent") && check_env(startup, "CHILD_ONLY", NULL));

    /* A child that exits from a nested frame. Its stream was closed by exit, and the
     * status is collected once. */
    XilaFile *exit_output = NULL;
    CHECK(api->file_open(startup->home, "exit-output", 11, XILA_OPEN_WRITE | XILA_OPEN_CREATE,
                         &exit_output) == XILA_STATUS_OK);
    XilaSpawn exit_spawn = spawn_of(startup);
    exit_spawn.standard_output = exit_output;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &exit_spawn, &child) == XILA_STATUS_OK);
    CHECK(child != NULL);
    CHECK(api->file_close(exit_output) == XILA_STATUS_OK); /* The child holds a duplicate. */
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_OK && status == 42);
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_NO_CHILD);
    CHECK(api->process_wait(me, child, XILA_WAIT_NO_HANG, NULL) == XILA_STATUS_NO_CHILD);
    CHECK(status == 42);
    CHECK(api->process_child_close(child) == XILA_STATUS_OK);
    CHECK(api->process_child_close(NULL) == XILA_STATUS_OK);
    CHECK(check_env(startup, "CHILD_ONLY", NULL));

    /* A child's identifier is the one it reports itself, and it stays readable after the
     * wait until the handle is closed. */
    XilaSpawn identify_spawn = spawn_of(startup);
    uint32_t child_id = 0;
    CHECK(api->process_spawn(me, startup->home, "identify", 8, &identify_spawn, &child) ==
          XILA_STATUS_OK);
    CHECK(api->process_child_id(child, &child_id) == XILA_STATUS_OK);
    CHECK(child_id != 0 && child_id != id);
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_OK);
    CHECK((uint32_t)status == child_id);
    child_id = 0;
    CHECK(api->process_child_id(child, &child_id) == XILA_STATUS_OK && (uint32_t)status == child_id);
    CHECK(api->process_child_id(child, NULL) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_child_id(NULL, &child_id) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_child_close(child) == XILA_STATUS_OK);

    /* Explicit environments replace the copy: later entries win. */
    XilaString environment[3] = {TEXT("ONLY=1"), TEXT("DUP=a"), TEXT("DUP=b")};
    XilaSpawn explicit_spawn = spawn_of(startup);
    explicit_spawn.environment = environment;
    explicit_spawn.environment_count = 3;
    CHECK(run_child(startup, "explicit", &explicit_spawn) == 9);
    XilaSpawn empty_spawn = spawn_of(startup);
    empty_spawn.environment = environment; /* Non-null, so explicit, with no entries. */
    CHECK(run_child(startup, "empty", &empty_spawn) == 8);
    XilaSpawn negative_spawn = spawn_of(startup);
    CHECK(run_child(startup, "negative", &negative_spawn) == -3);

    /* A running child: no-hang wait would block, a sibling cannot wait for it, and
     * waiting suspends this coroutine until it exits. */
    XilaSpawn slow_spawn = spawn_of(startup);
    CHECK(api->process_spawn(me, startup->home, "slow", 4, &slow_spawn, &child) == XILA_STATUS_OK);
    status = 123;
    CHECK(api->process_wait(me, child, XILA_WAIT_NO_HANG, &status) == XILA_STATUS_WOULD_BLOCK);
    CHECK(status == 123);
    CHECK(api->process_wait(me, child, 4, &status) == XILA_STATUS_INVALID_ARGUMENT);
    shared_child = child;
    shared_process = me;
    XilaSpawn steal_spawn = spawn_of(startup);
    steal_spawn.home = startup->home; /* The sibling needs a directory to name the program. */
    CHECK(run_child(startup, "steal", &steal_spawn) == 0);
    shared_child = NULL;
    shared_process = NULL;
    CHECK(api->process_wait(me, child, 0, &status) == XILA_STATUS_OK && status == 5);
    CHECK(api->process_child_close(child) == XILA_STATUS_OK);
    /* A child closed while running is detached. */
    CHECK(api->process_spawn(me, startup->home, "slow", 4, &slow_spawn, &child) == XILA_STATUS_OK);
    CHECK(api->process_child_close(child) == XILA_STATUS_OK);

    /* Spawn errors leave the output untouched. */
    child = NULL;
    CHECK(api->process_spawn(me, startup->home, "missing", 7, &spawn, &child) == XILA_STATUS_NOT_FOUND);
    CHECK(api->process_spawn(NULL, startup->home, "exit", 4, &spawn, &child) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_spawn(me, NULL, "exit", 4, &spawn, &child) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_spawn(me, startup->home, "exit", 4, NULL, &child) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &spawn, NULL) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_spawn(me, startup->home, "", 0, &spawn, &child) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_spawn(me, startup->home, "/exit", 5, &spawn, &child) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_spawn(me, startup->home, "../exit", 7, &spawn, &child) == XILA_STATUS_INVALID_ARGUMENT);
    XilaSpawn bad = spawn;
    bad.size = sizeof(bad) - 1;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    bad = spawn;
    bad.flags = 1;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    bad = spawn;
    bad.standard_error = NULL;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    bad = spawn;
    bad.environment_count = 1; /* With a null array. */
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    XilaString malformed[1] = {TEXT("NOEQUALS")};
    bad = spawn;
    bad.environment = malformed;
    bad.environment_count = 1;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    XilaString nameless[1] = {TEXT("=value")};
    bad.environment = nameless;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    XilaString invalid_argument[1] = {{"\xff", 1}};
    bad = spawn;
    bad.arguments = invalid_argument;
    bad.argument_count = 1;
    CHECK(api->process_spawn(me, startup->home, "exit", 4, &bad, &child) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(child == NULL);
    /* A directory capability without the READ right cannot name an executable. */
    XilaDirectory *write_only = NULL;
    CHECK(api->directory_open(startup->home, ".", 1, XILA_RIGHT_WRITE, &write_only) == XILA_STATUS_OK);
    CHECK(api->process_spawn(me, write_only, "exit", 4, &spawn, &child) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(api->directory_close(write_only) == XILA_STATUS_OK);
    CHECK(child == NULL);

    /* Wait argument errors, and the capability checks of exit. */
    CHECK(api->process_wait(me, NULL, 0, &status) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_wait(NULL, shared_child, 0, &status) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->process_exit(NULL, 0) == XILA_STATUS_INVALID_ARGUMENT);
    return 0;
}
