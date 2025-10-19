cmake_minimum_required(VERSION 4.0)

project(unsafe CXX)
set(CMAKE_CXX_STANDARD 17)
set(PROJECT_VERSION "0.0.1")

find_package(OpenMP REQUIRED)

file(GLOB SOURCE_FILES "src/*.cc")
add_library(${PROJECT_NAME} ${SOURCE_FILES})

target_link_libraries(${PROJECT_NAME} PRIVATE OpenMP::OpenMP_CXX)

install(TARGETS unsafe DESTINATION .)
