cmake_minimum_required(VERSION 4.0)

project(unsafe CXX)
set(CMAKE_CXX_STANDARD 17)
set(PROJECT_VERSION "0.0.1")

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -fno-exceptions -fno-rtti")
set(CMAKE_OSX_ARCHITECTURES "arm64")
set(CMAKE_OSX_DEPLOYMENT_TARGET "15.0")

find_package(OpenMP REQUIRED)

file(GLOB SOURCE_FILES "src/*.cc")
add_library(${PROJECT_NAME} STATIC ${SOURCE_FILES})

target_link_libraries(${PROJECT_NAME} PRIVATE ${OpenMP_CXX_LIBRARIES})

install(TARGETS unsafe DESTINATION .)
