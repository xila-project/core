cmake_minimum_required(VERSION 3.20)
# hexdump: canonical hex + ASCII dump (and its -r reverse) for Xila native
# applications. Configure with the SDK's toolchain file:
#   cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_hexdump C)

# The object library lets another project reuse the compiled utility; the
# executable produces hexdump.elf (CMAKE_EXECUTABLE_SUFFIX_C is set by the SDK's
# toolchain file).
add_library(hexdump_objects OBJECT hexdump.c)
add_executable(hexdump $<TARGET_OBJECTS:hexdump_objects>)
