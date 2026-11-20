cmake_minimum_required(VERSION 3.20)
# Test program; not shipped in the SDK. Configure with the SDK's toolchain file:
# cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_exit_suspended C)

add_executable(exit_suspended exit_suspended.c)
