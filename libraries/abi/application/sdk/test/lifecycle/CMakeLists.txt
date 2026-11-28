cmake_minimum_required(VERSION 3.20)
project(xila_lifecycle C)

add_executable(lifecycle lifecycle.c)
add_executable(lifecycle-direct direct.c)

if(CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64")
  # Compile C++ with the SDK's C flags. Link the object with the existing C rule.
  set(CMAKE_CXX_COMPILER "${CMAKE_C_COMPILER}")
  set(CMAKE_CXX_FLAGS_INIT "${CMAKE_C_FLAGS_INIT}")
  enable_language(CXX)
  add_library(lifecycle-cxx-object OBJECT lifecycle.cpp)
  target_compile_options(lifecycle-cxx-object PRIVATE -Os -fno-exceptions -fno-rtti)
  add_executable(lifecycle-cxx $<TARGET_OBJECTS:lifecycle-cxx-object>)
  set_target_properties(lifecycle-cxx PROPERTIES LINKER_LANGUAGE C)
endif()
