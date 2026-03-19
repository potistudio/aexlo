cmake_minimum_required(VERSION 3.21)

project(test_e2e_clients LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

set(E2E_HEADERS_DIR ${CMAKE_CURRENT_SOURCE_DIR}/headers)
set(
	SDK_SOURCES
	${E2E_HEADERS_DIR}/utils/AEGP_SuiteHandler.cpp
	${E2E_HEADERS_DIR}/utils/MissingSuiteError.cpp
	${E2E_HEADERS_DIR}/utils/Smart_Utils.cpp
)

function(add_e2e_client TARGET_NAME ENTRY_SOURCE)
	add_library(${TARGET_NAME} SHARED ${ENTRY_SOURCE} ${SDK_SOURCES})
	target_compile_definitions(${TARGET_NAME} PRIVATE _WINDOWS)
	target_include_directories(
		${TARGET_NAME}
		PRIVATE
		${E2E_HEADERS_DIR}
		${E2E_HEADERS_DIR}/utils
		${E2E_HEADERS_DIR}/SP
	)
	install(TARGETS ${TARGET_NAME} DESTINATION .)
endfunction()

add_subdirectory(about_message_test)
add_subdirectory(pointer_validation_test)
