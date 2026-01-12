cmake_minimum_required(VERSION 3.20)
project(quantization-mcp VERSION 1.0.0 LANGUAGES CXX)

# 设置C++17标准（兼容性更好）
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# 编译选项
if(MSVC)
    add_compile_options(/W4 /WX)
else()
    add_compile_options(-Wall -Wextra -Wpedantic -Werror)
endif()

# 查找依赖库
find_package(CURL REQUIRED)
find_package(nlohmann_json 3.11.0 REQUIRED)

# 包含目录
include_directories(${CMAKE_SOURCE_DIR}/include)

# 技术指标源文件
set(INDICATOR_SOURCES
    src/indicators/sma.cpp
    src/indicators/ema.cpp
    src/indicators/rsi.cpp
    src/indicators/macd.cpp
    src/indicators/bollinger_bands.cpp
    src/indicators/stochastic.cpp
    src/indicators/atr.cpp
    src/indicators/adx.cpp
    src/indicators/cci.cpp
    src/indicators/williams_r.cpp
    src/indicators/obv.cpp
)

# 核心源文件
set(CORE_SOURCES
    src/network_data_source.cpp
    src/mcp_server.cpp
)

# 创建可执行文件
add_executable(quantization-mcp
    src/main.cpp
    ${CORE_SOURCES}
    ${INDICATOR_SOURCES}
)

# 链接库
target_link_libraries(quantization-mcp
    PRIVATE
        CURL::libcurl
        nlohmann_json::nlohmann_json
)

# 安装规则
install(TARGETS quantization-mcp
    RUNTIME DESTINATION bin
)

# 可选：创建静态库供其他项目使用
add_library(quantization-indicators STATIC
    ${INDICATOR_SOURCES}
)

target_include_directories(quantization-indicators
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>
)

install(TARGETS quantization-indicators
    ARCHIVE DESTINATION lib
)

install(DIRECTORY include/
    DESTINATION include
    FILES_MATCHING PATTERN "*.hpp"
)

# 测试（可选）
option(BUILD_TESTS "Build tests" OFF)
if(BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()
