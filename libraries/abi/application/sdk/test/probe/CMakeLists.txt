cmake_minimum_required(VERSION 3.20)
# Test program for the SDK's POSIX layer; not shipped in the SDK. Configure with the SDK's
# toolchain file: cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_probe C)

add_executable(probe probe.c)
# Only the main probe needs raw startup and heap access for interoperation and allocation tests.
target_link_options(probe PRIVATE --wrap=xila_main --wrap=xila_heap_allocate --wrap=xila_heap_free
                                  --wrap=xila_heap_reallocate --wrap=xila_env_entry)

# Calls the environment accessors without referencing the optional environ table.
add_executable(no-environ no-environ.c)
