cmake_minimum_required(VERSION 3.20)

project(tg_indicators VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(Protobuf REQUIRED)
find_package(PkgConfig REQUIRED)
pkg_check_modules(GRPC REQUIRED grpc++ grpc)
pkg_check_modules(GTEST_PKG QUIET gtest)
find_package(GTest QUIET)

find_program(GRPC_CPP_PLUGIN_EXECUTABLE grpc_cpp_plugin
  PATHS /usr/local/bin
  REQUIRED)

set(REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../..")
set(PROTO_ROOT "${REPO_ROOT}/crates/tg-contracts/proto")
set(CONTRACTS_PROTO "${PROTO_ROOT}/tg/v1/contracts.proto")
set(GENERATED_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated")
set(GENERATED_PROTO_SRCS
  "${GENERATED_DIR}/tg/v1/contracts.pb.cc"
  "${GENERATED_DIR}/tg/v1/contracts.grpc.pb.cc")
set(GENERATED_PROTO_HDRS
  "${GENERATED_DIR}/tg/v1/contracts.pb.h"
  "${GENERATED_DIR}/tg/v1/contracts.grpc.pb.h")

add_custom_command(
  OUTPUT ${GENERATED_PROTO_SRCS} ${GENERATED_PROTO_HDRS}
  COMMAND ${CMAKE_COMMAND} -E make_directory "${GENERATED_DIR}"
  COMMAND ${Protobuf_PROTOC_EXECUTABLE}
    --proto_path="${PROTO_ROOT}"
    --cpp_out="${GENERATED_DIR}"
    --grpc_out="${GENERATED_DIR}"
    --plugin=protoc-gen-grpc="${GRPC_CPP_PLUGIN_EXECUTABLE}"
    "${CONTRACTS_PROTO}"
  DEPENDS "${CONTRACTS_PROTO}"
  COMMENT "Generating C++ protobuf and gRPC sources from tg-contracts")

add_library(tg_contracts_proto STATIC ${GENERATED_PROTO_SRCS} ${GENERATED_PROTO_HDRS})
target_include_directories(tg_contracts_proto PUBLIC
  "${GENERATED_DIR}"
  ${Protobuf_INCLUDE_DIRS}
  ${GRPC_INCLUDE_DIRS})
target_link_libraries(tg_contracts_proto PUBLIC
  ${Protobuf_LIBRARIES}
  ${GRPC_LIBRARIES})
target_compile_options(tg_contracts_proto PRIVATE -Wall -Wextra)

set(INDICATOR_SOURCES
  src/indicator_service.cpp
  src/indicators/registry.cpp
  src/indicators/sma.cpp
  src/indicators/ema.cpp
  src/indicators/macd.cpp
  src/indicators/rsi.cpp
  src/indicators/bollinger_bands.cpp
  src/indicators/atr.cpp
  src/indicators/adx.cpp
  src/indicators/cci.cpp
  src/indicators/stochastic.cpp
  src/indicators/williams_r.cpp
  src/indicators/obv.cpp)

add_library(tg_indicators_core STATIC ${INDICATOR_SOURCES})
target_include_directories(tg_indicators_core PUBLIC
  "${CMAKE_CURRENT_SOURCE_DIR}/include"
  "${GENERATED_DIR}")
target_link_libraries(tg_indicators_core PUBLIC tg_contracts_proto)
target_compile_options(tg_indicators_core PRIVATE -Wall -Wextra -Werror)

add_executable(tg-indicators src/main.cpp)
target_link_libraries(tg-indicators PRIVATE tg_indicators_core)
target_compile_options(tg-indicators PRIVATE -Wall -Wextra -Werror)

enable_testing()

add_executable(tg_indicators_tests tests/indicator_tests.cpp)
target_link_libraries(tg_indicators_tests PRIVATE tg_indicators_core)
if(TARGET GTest::gtest)
  target_link_libraries(tg_indicators_tests PRIVATE GTest::gtest)
elseif(GTEST_PKG_FOUND)
  target_include_directories(tg_indicators_tests PRIVATE ${GTEST_PKG_INCLUDE_DIRS})
  target_link_libraries(tg_indicators_tests PRIVATE ${GTEST_PKG_LIBRARIES})
else()
  message(FATAL_ERROR "GTest was not found")
endif()
target_link_libraries(tg_indicators_tests PRIVATE pthread)
target_compile_options(tg_indicators_tests PRIVATE -Wall -Wextra -Werror)

add_test(NAME tg_indicators_tests COMMAND tg_indicators_tests)

