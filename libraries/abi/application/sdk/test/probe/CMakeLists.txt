cmake_minimum_required(VERSION 3.20)
# Test program for the SDK's POSIX layer; not shipped in the SDK. Configure with the SDK's
# toolchain file: cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_probe C)

add_executable(probe probe.c)
# Only the probe needs the raw startup capability to test ABI/POSIX interoperation.
target_link_options(probe PRIVATE --wrap=xila_main)
