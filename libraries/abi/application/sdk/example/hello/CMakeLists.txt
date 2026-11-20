cmake_minimum_required(VERSION 3.20)
# Configure with the SDK's toolchain file:
#   cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
project(xila_hello C)

add_executable(hello hello.c)

# Configure with -DXILA_PRINTF_FORMAT=double to also check floating-point printf.
if(XILA_PRINTF_FORMAT STREQUAL "double")
  target_compile_definitions(hello PRIVATE HELLO_FLOAT)
endif()
