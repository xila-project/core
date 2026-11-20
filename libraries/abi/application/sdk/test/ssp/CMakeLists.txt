cmake_minimum_required(VERSION 3.20)
# Stack protector test program; not shipped in the SDK. Configure with the SDK's toolchain
# file: cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_ssp C)

add_executable(ssp ssp.c)
# The toolchain file builds without the protector; the application opts in, and the SDK's
# __stack_chk_guard and __stack_chk_fail satisfy the references.
target_compile_options(ssp PRIVATE -fstack-protector-strong)
