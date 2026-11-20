cmake_minimum_required(VERSION 3.20)
# Graphics test program; not shipped in the SDK. Configure with the SDK's toolchain file:
# cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_graphics C)

add_executable(graphics graphics.c)
