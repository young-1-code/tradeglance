#!/bin/bash

# 构建脚本 - 自动化编译过程

set -e  # 遇到错误立即退出

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== Quantization MCP Server 构建脚本 ===${NC}\n"

# 检查依赖
echo "检查依赖..."

# 检查编译器
if ! command -v g++ &> /dev/null && ! command -v clang++ &> /dev/null; then
    echo -e "${RED}错误: 未找到C++编译器 (g++ 或 clang++)${NC}"
    exit 1
fi

# 检查CMake
if ! command -v cmake &> /dev/null; then
    echo -e "${RED}错误: 未找到CMake${NC}"
    echo "请安装CMake: sudo apt install cmake"
    exit 1
fi

# 检查libcurl
if ! pkg-config --exists libcurl; then
    echo -e "${YELLOW}警告: 未找到libcurl开发库${NC}"
    echo "请安装: sudo apt install libcurl4-openssl-dev"
fi

# 检查nlohmann-json
if [ ! -f "/usr/include/nlohmann/json.hpp" ] && [ ! -f "/usr/local/include/nlohmann/json.hpp" ]; then
    echo -e "${YELLOW}警告: 未找到nlohmann-json库${NC}"
    echo "请安装: sudo apt install nlohmann-json3-dev"
fi

echo -e "${GREEN}依赖检查完成${NC}\n"

# 创建构建目录
BUILD_DIR="build"
if [ -d "$BUILD_DIR" ]; then
    echo "清理旧的构建目录..."
    rm -rf "$BUILD_DIR"
fi

mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# 配置CMake
echo -e "\n${GREEN}配置CMake...${NC}"
cmake .. \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_STANDARD=23 \
    -DCMAKE_EXPORT_COMPILE_COMMANDS=ON

# 编译
echo -e "\n${GREEN}开始编译...${NC}"
NPROC=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
cmake --build . --config Release -j$NPROC

# 检查编译结果
if [ -f "quantization-mcp" ]; then
    echo -e "\n${GREEN}✓ 编译成功!${NC}"
    echo -e "可执行文件位置: ${GREEN}$BUILD_DIR/quantization-mcp${NC}"

    # 显示文件信息
    ls -lh quantization-mcp

    echo -e "\n${GREEN}使用方法:${NC}"
    echo "  ./build/quantization-mcp"
    echo "  或"
    echo "  export MARKET_DATA_API=http://your-api-endpoint.com/api"
    echo "  ./build/quantization-mcp"
else
    echo -e "\n${RED}✗ 编译失败${NC}"
    exit 1
fi

# 可选：运行测试
if [ "$1" == "--test" ]; then
    echo -e "\n${GREEN}运行测试...${NC}"
    ctest --output-on-failure
fi

echo -e "\n${GREEN}构建完成!${NC}"
